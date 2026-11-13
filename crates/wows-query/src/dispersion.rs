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
use wows_core::Version;
use wows_core::units::Km;
use wows_core::units::Meters;
use wows_core::units::Percent;
use wows_ship_stats::Triggers;
use wowsunpack::game_params::ttx::ShipUpgradeSelection;
use wowsunpack::game_params::ttx::constants::DispersionCurve;
use wowsunpack::game_params::ttx::constants::clamped_dispersion_coeff;
use wowsunpack::game_params::ttx::factories::cap_artillery_range_km;
use wowsunpack::game_params::ttx::modifiers::ModifierBundle;
use wowsunpack::game_params::ttx::orchestration::ArtilleryDispersion;
use wowsunpack::game_params::ttx::orchestration::artillery_dispersion;
use wowsunpack::game_params::ttx::orchestration::artillery_sigma;
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
use crate::ships::ShipClass;
use crate::ships::ShipStatus;
use crate::ships::ShipSummary;
use crate::ships::Tier;

/// Secondary accuracy against the priority target within secondary range.
const PRIORITY_TARGET_ACCURACY: &str = "GSPriorityTargetIdealRadius";

/// Condition of the priority target accuracy of Manual Secondary Battery Aiming.
const PRIORITY_TARGET_CONDITION: &str = "at the priority target within secondary range, after sustained fire";

const FORMULA_NOTE: &str = "horizontal_m = 30 x (minRadius + X_m / 30 x (idealRadius - minRadius) / idealDistance), X_m the firing distance in meters and both radii times the accuracy coefficient (Aiming Systems Modification 1 and the like); it depends only on the firing distance. vertical_m = horizontal_m x vertical_ratio, where vertical_ratio runs linearly from radiusOnZero at 0 km to radiusOnDelim at delim x max range, then to radiusOnMax at max range. A longer max range lowers X / max range, so at the same X the ratio, and with it vertical dispersion, falls whenever radiusOnZero < radiusOnDelim < radiusOnMax. At max range horizontal_m is the port's maximum dispersion.";
const MAIN_RANGE_NOTE: &str = "max range is the modified main battery range: fire control module, upgrades, captain skills and Spotting Aircraft all lengthen it. The game client passes the modified range to the gun, and replay measurements (Colbert at 6 km, 13.8 km against 19.2 km range with range skill and upgrade) showed the predicted 16% smaller vertical spread. Spotting Aircraft lengthens the same range value, so it is assumed to act the same way; that case was not measured.";
const SECONDARY_NOTE: &str = "Secondary max range is the base secondary range times the secondary range modifiers. The accuracy coefficient is the one the game applies in battle: GSIdealRadius times GSIdealRadiusHighLevel on tier 7 and higher ships or GSIdealRadiusLowLevel below, times GSPriorityTargetIdealRadius in rows whose condition names the priority target; the port card applies GSIdealRadius alone. Manual secondary control modes are not modeled.";
const SIGMA_NOTE: &str = "Shell spread inside the ellipse (getRandomLastPoints and gaussClamped in the game scripts): each shell takes a point on the ellipse edge at a random angle, scaled by r, a sample of N(0, 1 / sigmaCount) redrawn from Uniform[-1, 1] when it falls outside [-1, 1]. So the share of shells inside the ellipse scaled by t is P(|r| <= t) = erf(t x sigmaCount / sqrt 2) + (1 - erf(sigmaCount / sqrt 2)) x t. Above the waterline the game also compresses the vertical offset (community analysis gives 10 x log(0.1 Y + 1)); the inner ellipses here leave that out, so their upper half overstates the spread.";
const TAPER_NOTE: &str = "Below the gun's taperDist, 4 to 5 km for most guns, the game narrows horizontal dispersion further toward the muzzle; values at these distances do not apply it and overstate dispersion.";
fn default_dispersion_range() -> Km {
    Km::new(12.0)
}
/// Distances below this may fall under a gun's taperDist.
const TAPER_REACH_KM: f32 = 5.0;

/// Arguments for `dispersion_at_range`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DispersionAtRangeArgs {
    /// Ship name, e.g. "Yamato".
    pub ship: String,

    /// Firing distance in km, more than 0. Defaults to 12.
    #[serde(default = "default_dispersion_range")]
    pub range_km: Km,

    /// "main_battery" (default) or "secondary".
    pub battery: Option<DispersionBattery>,

    /// Modules to mount instead of stock, e.g. ["Hull B"]. Omit for stock.
    pub modules: Option<Vec<String>>,

    /// The build asked about, which the conclusion leads with: "range mod",
    /// "spotter", "accuracy mod", or an upgrade, captain skill, module or
    /// consumable name as listed in builds.
    pub build: Option<String>,

    /// Ranks the ship among every released ship of a class and tier range at
    /// the same distance, e.g. {"class": "battleship", "min_tier": 10, "max_tier": 10}.
    pub compare: Option<CompareGroup>,
}

/// The ships to rank against.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CompareGroup {
    pub class: ShipClass,
    pub min_tier: Tier,
    pub max_tier: Tier,
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

/// `sigmaCount` of a battery: higher draws more shells toward the aim point.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct SigmaCount(f32);

impl SigmaCount {
    pub fn new(value: f32) -> Self {
        Self(value)
    }

    pub fn value(self) -> f32 {
        self.0
    }

    /// Share of shells landing inside the ellipse scaled by `fraction`, from
    /// 0 to 1. Each shell takes a point on the ellipse edge scaled by r, a
    /// sample of N(0, 1 / sigmaCount) redrawn from Uniform[-1, 1] when it
    /// falls outside [-1, 1], so P(|r| <= t) = erf(t x sigmaCount / sqrt 2)
    /// + (1 - erf(sigmaCount / sqrt 2)) x t.
    pub fn share_within(self, fraction: RadiusFraction) -> f64 {
        let sigma = f64::from(self.0);
        let t = f64::from(fraction.0).clamp(0.0, 1.0);
        let within = |x: f64| erf(x * sigma / std::f64::consts::SQRT_2);

        within(t) + (1.0 - within(1.0)) * t
    }

    /// The radius fraction inside which `share` (0 to 1) of the shells land.
    pub fn fraction_holding(self, share: f64) -> RadiusFraction {
        /// Halvings of [0, 1]; enough for f32 precision.
        const STEPS: u32 = 40;

        let (mut low, mut high) = (0.0_f32, 1.0_f32);
        for _ in 0..STEPS {
            let mid = (low + high) / 2.0;
            if self.share_within(RadiusFraction(mid)) < share {
                low = mid;
            } else {
                high = mid;
            }
        }

        RadiusFraction((low + high) / 2.0)
    }
}

/// A fraction of the ellipse semi-axes, 0 at the aim point and 1 on the edge.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct RadiusFraction(f32);

impl RadiusFraction {
    pub fn value(self) -> f32 {
        self.0
    }
}

/// How shells spread inside the ellipse under one sigmaCount.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ShellSpread {
    pub sigma_count: SigmaCount,

    /// Half of the shells land inside the ellipse scaled by this fraction.
    pub half_of_shells_radius_fraction: RadiusFraction,

    /// 90% of the shells land inside the ellipse scaled by this fraction.
    pub ninety_pct_of_shells_radius_fraction: RadiusFraction,

    /// Share of shells inside the ellipse scaled by one half.
    pub inner_half_share_pct: Percent,
}

impl ShellSpread {
    fn from_sigma(sigma_count: SigmaCount) -> Self {
        Self {
            sigma_count: SigmaCount(round_to(sigma_count.0, 1000.0)),
            half_of_shells_radius_fraction: RadiusFraction(round_to(sigma_count.fraction_holding(0.5).0, 1000.0)),
            ninety_pct_of_shells_radius_fraction: RadiusFraction(round_to(sigma_count.fraction_holding(0.9).0, 1000.0)),
            inner_half_share_pct: Percent::new(round_to(
                (sigma_count.share_within(RadiusFraction(0.5)) * 100.0) as f32,
                10.0,
            )),
        }
    }
}

/// Semi-axes of an ellipse inside the dispersion ellipse that holds a share
/// of the shells.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct InnerEllipse {
    pub horizontal_m: Meters,
    pub vertical_m: Meters,
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
    /// The distance the ellipse is taken at when range_km is slightly past
    /// the build's max range; absent when it is taken at range_km.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evaluated_at_km: Option<Km>,

    pub horizontal_m: Meters,
    pub vertical_m: Meters,
    pub vertical_ratio: VerticalRatio,

    /// Against the base build, absent on the base row or when the base build
    /// does not reach the distance.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub horizontal_change_pct: Option<Percent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vertical_change_pct: Option<Percent>,

    /// The inner ellipses holding half and 90% of the shells, absent
    /// without a sigmaCount.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub half_of_shells_within: Option<InnerEllipse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ninety_pct_of_shells_within: Option<InnerEllipse>,
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

    /// Absent when game data gives the battery no sigmaCount.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shells: Option<ShellSpread>,

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
    /// Game version that supplied an explicitly selected historical mechanic.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mechanics_version: Option<Version>,
    pub conclusion: String,
    pub guns: Vec<GunCurve>,
    /// The row of the build asked about.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub headline: Option<String>,

    pub builds: Vec<DispersionRow>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<GroupComparison>,

    pub notes: Vec<&'static str>,
}

/// The ship's standing among a class and tier range at one distance. Each
/// ship is taken on the build with the shortest range that reaches the
/// distance, or on its longest range build when the distance is slightly
/// past it.
#[derive(Debug, Clone, Serialize)]
pub struct GroupComparison {
    pub class: ShipClass,
    pub min_tier: Tier,
    pub max_tier: Tier,

    /// The ship's own build in the ranking.
    pub ship_build: String,

    /// Ships ranked, the named ship included.
    pub ships_ranked: usize,

    /// Ships whose range falls short of the distance on every build.
    pub ships_out_of_reach: usize,

    /// Up to ten of them.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub out_of_reach: Vec<String>,

    pub horizontal: Standing,
    pub vertical: Standing,

    /// Horizontal semi-axis of the inner ellipse holding half of the shells.
    /// Absent when the battery has no sigmaCount.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub half_of_shells_horizontal: Option<Standing>,
}

/// One measure across the group, smaller is tighter.
#[derive(Debug, Clone, Serialize)]
pub struct Standing {
    /// 1 is the tightest.
    pub rank: usize,
    pub value_m: Meters,
    pub median_m: Meters,
    pub tightest: GroupShip,
    pub widest: GroupShip,
}

#[derive(Debug, Clone, Serialize)]
pub struct GroupShip {
    pub ship: String,
    pub build: String,
    pub value_m: Meters,
}

/// What lengthens range or tightens accuracy.
#[derive(Debug, Clone, PartialEq)]
enum Source {
    Item(Item),

    /// A fire control module, by its `ShipUpgradeInfo` key.
    FireControl(String),

    /// A consumable's `artilleryDistCoeff`, e.g. Spotting Aircraft.
    RangeConsumable(RangeCoefficient),

    /// The ship's special ability, switched on.
    SpecialAbility,
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
    sigma: Option<SigmaCount>,
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
    /// The distance the ellipse is taken at.
    at: Km,
    horizontal: Meters,
    vertical: Meters,
    ratio: VerticalRatio,
}

/// The rows of one ship, with the source of each single-candidate row by
/// row index.
struct ShipRows {
    rows: Vec<DispersionRow>,
    sources: Vec<(usize, Source)>,
    guns: Vec<GunCurve>,
}

impl ShipRows {
    /// The row `hint` names: "range mod", "spotter", "accuracy mod", "base",
    /// or a build label.
    fn resolve_build(&self, hint: &str) -> Result<usize, QueryError> {
        const RANGE: &[&str] = &["range mod", "range module", "range upgrade", "range modification"];
        const SPOTTER: &[&str] =
            &["spotter", "spotter plane", "spotting aircraft", "spotting plane", "spotter aircraft"];
        const ACCURACY: &[&str] =
            &["accuracy mod", "accuracy module", "accuracy upgrade", "asm", "asm1", "aiming systems mod"];

        let wanted = hint.trim().to_lowercase();
        let lasting_source = |accepts: &dyn Fn(&Source, RowKind) -> bool| {
            self.sources
                .iter()
                .find(|(index, source)| {
                    self.rows[*index].condition.is_none() && accepts(source, self.rows[*index].kind)
                })
                .map(|(index, _)| *index)
        };
        let found = if wanted == "base" || wanted == "stock" {
            Some(0)
        } else if RANGE.contains(&wanted.as_str()) {
            lasting_source(&|source, kind| {
                matches!(source, Source::Item(Item::Upgrade(_)))
                    && matches!(kind, RowKind::Range | RowKind::RangeAndAccuracy)
            })
        } else if SPOTTER.contains(&wanted.as_str()) {
            lasting_source(&|source, _| matches!(source, Source::RangeConsumable(_)))
        } else if ACCURACY.contains(&wanted.as_str()) {
            lasting_source(&|source, kind| {
                matches!(source, Source::Item(Item::Upgrade(_))) && kind == RowKind::Accuracy
            })
        } else {
            let label = |index: usize| self.rows[index].build.to_lowercase();
            let indices = 0..self.rows.len();
            indices
                .clone()
                .find(|index| label(*index) == wanted)
                .or_else(|| indices.clone().find(|index| label(*index).contains(&wanted)))
                .or_else(|| indices.clone().skip(1).find(|index| wanted.contains(&label(*index))))
        };

        found.ok_or(QueryError::InvalidArguments {
            reason: "build matches no row of builds; name a listed upgrade, skill, module or consumable, or \"range mod\", \"spotter\" or \"accuracy mod\"",
        })
    }
}

/// The values of one ship in a group comparison.
struct GroupEntry {
    build: String,
    horizontal: f32,
    vertical: f32,
    half_of_shells: Option<f32>,
}

impl GroupEntry {
    /// `None` when the row's first mount does not reach the distance.
    fn from_row(row: &DispersionRow) -> Option<Self> {
        let ellipse = first_ellipse(row)?;

        Some(Self {
            build: row.build.clone(),
            horizontal: ellipse.horizontal_m.value(),
            vertical: ellipse.vertical_m.value(),
            half_of_shells: ellipse.half_of_shells_within.as_ref().map(|half| half.horizontal_m.value()),
        })
    }
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

    /// The secondary component's sigmaCount; no modifier changes it.
    secondary_sigma: Option<SigmaCount>,
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
                let max_range = cap_artillery_range_km(profile.max_range.value() * range.0, bundle);
                let profile = ArtilleryDispersion { max_range: Km::new(max_range), ..profile };

                let sigma = artillery_sigma(&self.ship.param, selection, bundle).map(SigmaCount);

                Some(vec![MountProfile { mount: "main battery".to_owned(), profile, sigma }])
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
                        sigma: self.secondary_sigma,
                    })
                    .collect();

                (!profiles.is_empty()).then_some(profiles)
            }
        }
    }
}

fn dispersion_notes(kind: DispersionBattery, range: Km) -> Vec<&'static str> {
    let mut notes = vec![FORMULA_NOTE, SIGMA_NOTE];
    notes.push(match kind {
        DispersionBattery::MainBattery => MAIN_RANGE_NOTE,
        DispersionBattery::Secondary => SECONDARY_NOTE,
    });
    if range.value() < TAPER_REACH_KM {
        notes.push(TAPER_NOTE);
    }
    notes
}

impl GameData {
    /// Horizontal and vertical dispersion at one distance on the base build
    /// and with each range or accuracy modifier the ship can use.
    pub fn dispersion_at_range(&self, args: &DispersionAtRangeArgs) -> Result<DispersionAtRange, QueryError> {
        let range = args.range_km;
        if !range.value().is_finite() || range.value() <= 0.0 {
            return Err(QueryError::InvalidArguments { reason: "range_km must be more than 0" });
        }

        let mut interpretations = Interpretations::default();
        let ship = self.resolve_ship(&args.ship, &mut interpretations)?;
        let modules = self.resolve_modules(ship, args.modules.as_deref().unwrap_or_default())?;
        self.check_hull_fit(ship, &modules)?;
        let kind = args.battery.unwrap_or_default();

        let Some(computed) = self.dispersion_rows(ship, kind, &modules.selection, range)? else {
            return Err(QueryError::WeaponNotFound { ship: ship.name.clone(), weapon: kind.noun().to_owned() });
        };
        let headline = match args.build.as_deref() {
            Some(hint) => Some(computed.resolve_build(hint)?),
            None => None,
        };
        let group = match &args.compare {
            Some(compare) => Some(self.group_comparison(ship, kind, range, compare, &computed, headline)?),
            None => None,
        };

        let mut conclusion = match headline {
            Some(0) | None => conclusion(ship, kind, range, &computed.rows),
            Some(index) => build_comparison_sentence(ship, kind, range, &computed.rows[0], &computed.rows[index]),
        };
        if let Some(group) = &group {
            conclusion.push_str(&group_sentence(ship, range, group));
        }
        let notes = dispersion_notes(kind, range);

        Ok(DispersionAtRange {
            interpretations,
            ship: ship.summary(),
            modules: modules.chosen,
            battery: kind,
            range_km: range,
            mechanics_version: None,
            conclusion,
            guns: computed.guns,
            headline: headline.map(|index| computed.rows[index].build.clone()),
            builds: computed.rows,
            group,
            notes,
        })
    }

    /// Dispersion for this game-data build with the captain skill named by
    /// `args.build` selected from another build. Ship, module, and base-build
    /// data come from `self`; only the skill and its mechanics come from
    /// `skill_data`.
    pub fn dispersion_at_range_with_skill(
        &self,
        args: &DispersionAtRangeArgs,
        skill_data: &GameData,
    ) -> Result<DispersionAtRange, QueryError> {
        let range = args.range_km;
        if !range.value().is_finite() || range.value() <= 0.0 {
            return Err(QueryError::InvalidArguments { reason: "range_km must be more than 0" });
        }
        let skill_query = args.build.as_deref().ok_or(QueryError::InvalidArguments {
            reason: "build must name the captain skill selected from skill_data",
        })?;

        let mut interpretations = Interpretations::default();
        let ship = self.resolve_ship(&args.ship, &mut interpretations)?;
        let modules = self.resolve_modules(ship, args.modules.as_deref().unwrap_or_default())?;
        self.check_hull_fit(ship, &modules)?;
        let kind = args.battery.unwrap_or_default();
        let battery = self.dispersion_battery(ship, kind, &modules.selection)?;
        let stock_loadout = self.stock_loadout();
        let stock_effects = self.effective_modifiers(ship, &stock_loadout)?;
        let stock = stock_effects.build.modifiers.bundle();
        let Some(base) = battery.profiles(stock, &battery.selection, RangeCoefficient::NONE) else {
            return Err(QueryError::WeaponNotFound { ship: ship.name.clone(), weapon: kind.noun().to_owned() });
        };

        let (skill, _) =
            skill_data.resolve_skill_for_class(skill_query, ship.class, &ship.name, &mut interpretations)?;
        let skill_name = skill_data.skill_name(skill);
        let mut skill_loadout = stock_loadout;
        skill_loadout.skills.push(skill.clone());
        skill_loadout.triggers = Triggers::Skills(vec![skill.internal_name().clone()]);
        let skill_effects = self.effective_modifiers_at_version(ship, &skill_loadout, skill_data.version())?;
        let skill_build = skill_effects.build.modifiers.bundle();
        let Some(skill_profiles) = battery.profiles(skill_build, &battery.selection, RangeCoefficient::NONE) else {
            return Err(QueryError::WeaponNotFound { ship: ship.name.clone(), weapon: kind.noun().to_owned() });
        };
        let Some(row_kind) = change_kind(&base[0], &skill_profiles[0]) else {
            return Err(QueryError::InvalidArguments {
                reason: "the selected skill does not change this battery's range or accuracy",
            });
        };

        let condition = if kind == DispersionBattery::Secondary && skill_build.coef(PRIORITY_TARGET_ACCURACY) != 1.0 {
            Some(PRIORITY_TARGET_CONDITION.to_owned())
        } else {
            skill_data.skill_trigger_sentence(skill)
        };
        let mut skill_row = row(skill_name.clone(), row_kind, &skill_profiles, Some(&base), range);
        skill_row.condition = condition.clone();
        let mut rows = vec![row("base".to_owned(), RowKind::Base, &base, None, range), skill_row];

        // The skill on top of each range or accuracy build of this game build,
        // so "range mod and <skill>" has its own row.
        let planner = Planner::new(self, ship);
        let mut lasting = self.dispersion_candidates(&battery, &planner, stock, &base)?.lasting;
        // Today's copy of the selected skill would apply it twice.
        lasting.retain(|candidate| {
            !matches!(candidate.source, Source::Item(Item::Skill { index, .. })
                if planner.skill(index).internal_name() == skill.internal_name())
        });
        let singles = lasting.iter().map(|candidate| vec![candidate]);
        for chosen in singles.chain(combined_builds(&battery, &lasting)) {
            let recipe = Recipe::of(&battery, &chosen);
            let mut loadout = planner.loadout_of(&recipe.items, None);
            loadout.skills.push(skill.clone());
            loadout.triggers = Triggers::Skills(vec![skill.internal_name().clone()]);
            let effects = self.effective_modifiers_at_version(ship, &loadout, skill_data.version())?;
            let Some(profiles) =
                battery.profiles(effects.build.modifiers.bundle(), &recipe.selection, recipe.coefficient)
            else {
                continue;
            };
            let Some(combined_kind) = change_kind(&base[0], &profiles[0]) else {
                continue;
            };
            let mut combined =
                row(format!("{} + {skill_name}", build_label(&chosen)), combined_kind, &profiles, Some(&base), range);
            combined.condition = condition.clone();
            rows.push(combined);
        }

        let guns = base.iter().map(|mount| gun_curve(&mount.mount, &mount.profile.curve)).collect();
        let computed = ShipRows { rows, sources: Vec::new(), guns };
        let headline = 1;
        let group = match &args.compare {
            Some(compare) => Some(self.group_comparison(ship, kind, range, compare, &computed, Some(headline))?),
            None => None,
        };
        let mut conclusion = build_comparison_sentence(ship, kind, range, &computed.rows[0], &computed.rows[headline]);
        if let Some(condition) = &computed.rows[headline].condition {
            conclusion.push_str(" Condition: ");
            conclusion.push_str(condition);
            if !condition.ends_with(['.', '!', '?']) {
                conclusion.push('.');
            }
        }
        for combined in &computed.rows[headline + 1..] {
            conclusion.push_str(&combined_sentence(combined));
        }
        if let Some(group) = &group {
            conclusion.push_str(&group_sentence(ship, range, group));
        }
        let notes = dispersion_notes(kind, range);

        Ok(DispersionAtRange {
            interpretations,
            ship: ship.summary(),
            modules: modules.chosen,
            battery: kind,
            range_km: range,
            mechanics_version: Some(skill_data.version()),
            conclusion,
            guns: computed.guns,
            headline: Some(computed.rows[headline].build.clone()),
            builds: computed.rows,
            group,
            notes,
        })
    }

    /// The base row, one row per range or accuracy source and the combined
    /// builds. `None` when the ship lacks the battery or its dispersion data.
    fn dispersion_rows(
        &self,
        ship: &Ship,
        kind: DispersionBattery,
        selection: &ShipUpgradeSelection,
        range: Km,
    ) -> Result<Option<ShipRows>, QueryError> {
        let battery = self.dispersion_battery(ship, kind, selection)?;
        let planner = Planner::new(self, ship);
        let stock = planner.modifiers_with(&[])?;
        let Some(base) = battery.profiles(&stock, &battery.selection, RangeCoefficient::NONE) else {
            return Ok(None);
        };

        let candidates = self.dispersion_candidates(&battery, &planner, &stock, &base)?;
        let mut rows = vec![row("base".to_owned(), RowKind::Base, &base, None, range)];
        let mut sources = Vec::new();
        for candidate in candidates.lasting.iter().chain(&candidates.conditional) {
            let mut candidate_row =
                row(candidate.label.clone(), candidate.kind, &candidate.profiles, Some(&base), range);
            candidate_row.condition = candidate.condition.clone();
            sources.push((rows.len(), candidate.source.clone()));
            rows.push(candidate_row);
        }
        rows.extend(combined_rows(&battery, &planner, &candidates.lasting, &base, range)?);
        let guns = base.iter().map(|mount| gun_curve(&mount.mount, &mount.profile.curve)).collect();

        Ok(Some(ShipRows { rows, sources, guns }))
    }

    /// `ship`'s standing among the released ships of `compare` at `range`.
    fn group_comparison(
        &self,
        ship: &Ship,
        kind: DispersionBattery,
        range: Km,
        compare: &CompareGroup,
        own: &ShipRows,
        headline: Option<usize>,
    ) -> Result<GroupComparison, QueryError> {
        const OUT_OF_REACH_SHOWN: usize = 10;

        if compare.min_tier.0 > compare.max_tier.0 {
            return Err(QueryError::InvalidArguments { reason: "compare.min_tier must not exceed compare.max_tier" });
        }
        let own_row = match headline {
            Some(index) => Some(&own.rows[index]),
            None => ranked_row(&own.rows),
        };
        let Some(own_entry) = own_row.and_then(GroupEntry::from_row) else {
            return Err(QueryError::InvalidArguments {
                reason: "the ship cannot reach range_km on the build compared, so it has no rank",
            });
        };

        let members = self.ships.iter().filter(|other| {
            other.class == compare.class
                && (compare.min_tier.0..=compare.max_tier.0).contains(&other.tier.0)
                && other.is_regular()
                && other.status() == ShipStatus::Released
                && other.param.index() != ship.param.index()
        });
        let mut entries = vec![(ship.name.clone(), own_entry)];
        let mut out_of_reach = Vec::new();
        for other in members {
            let Some(rows) = self.dispersion_rows(other, kind, &ShipUpgradeSelection::stock(&other.param), range)?
            else {
                continue;
            };
            match ranked_row(&rows.rows).and_then(GroupEntry::from_row) {
                Some(entry) => entries.push((other.name.clone(), entry)),
                None => out_of_reach.push(other.name.clone()),
            }
        }
        out_of_reach.sort();

        Ok(GroupComparison {
            class: compare.class,
            min_tier: compare.min_tier,
            max_tier: compare.max_tier,
            ship_build: entries[0].1.build.clone(),
            ships_ranked: entries.len(),
            ships_out_of_reach: out_of_reach.len(),
            out_of_reach: out_of_reach.into_iter().take(OUT_OF_REACH_SHOWN).collect(),
            horizontal: standing(&entries, |entry| Some(entry.horizontal))
                .expect("the named ship has a horizontal value"),
            vertical: standing(&entries, |entry| Some(entry.vertical)).expect("the named ship has a vertical value"),
            half_of_shells_horizontal: standing(&entries, |entry| entry.half_of_shells),
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
            secondary_sigma: None,
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
        battery.secondary_sigma = atba.sigma_count.map(SigmaCount);

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
            let kind = change_kind(&base[0], &profiles[0])?;
            Some(Candidate { label, source, kind, condition: None, profiles })
        };
        let mut lasting = Vec::new();

        for &item in planner.items() {
            let bundle = planner.modifiers_with(&[item])?;
            let profiles = battery.profiles(&bundle, &battery.selection, RangeCoefficient::NONE);
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
            let idle = planner.modifiers_with(&[item])?;
            let active_profiles = battery.profiles(&active.bundle, &battery.selection, RangeCoefficient::NONE);
            let idle_profiles = battery.profiles(&idle, &battery.selection, RangeCoefficient::NONE);
            let (Some(active_profiles), Some(idle_profiles)) = (active_profiles, idle_profiles) else {
                continue;
            };
            if change_kind(&idle_profiles[0], &active_profiles[0]).is_none() {
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

        // A special ability that switches to another fire mode changes the guns, not their spread.
        match self.active_ability(battery.ship) {
            Ok(ability) if ability.switched.is_none() => {
                let active = Planner::new(self, battery.ship).with_ability(ability.modifiers, ability.burst);
                let outcome = active.stock()?;
                let profiles = battery.profiles(&outcome.bundle, &battery.selection, RangeCoefficient::NONE);
                if let Some(mut found) = profiles.and_then(|profiles| {
                    let kind = change_kind(&base[0], &profiles[0])?;
                    Some(Candidate {
                        label: ability.names.join(", "),
                        source: Source::SpecialAbility,
                        kind,
                        condition: None,
                        profiles,
                    })
                }) {
                    found.condition = Some("while the special ability is active".to_owned());
                    conditional.push(found);
                }
            }
            Ok(_) | Err(QueryError::NoSpecialAbility { .. }) => {
                // No ability, or one whose fire mode the base guns do not describe.
            }
            Err(error) => return Err(error),
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
/// range nor tightens accuracy. A higher sigmaCount counts as tighter.
fn change_kind(base: &MountProfile, candidate: &MountProfile) -> Option<RowKind> {
    /// Relative change below which two ranges or coefficients are equal.
    const TOLERANCE: f32 = 1e-4;

    let longer = candidate.profile.max_range.value() > base.profile.max_range.value() * (1.0 + TOLERANCE);
    let sharper = matches!(
        (base.sigma, candidate.sigma),
        (Some(base), Some(candidate)) if candidate.0 > base.0 * (1.0 + TOLERANCE)
    );
    let tighter = sharper || candidate.profile.ideal_radius_coef < base.profile.ideal_radius_coef * (1.0 - TOLERANCE);

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
    combined_builds(battery, candidates)
        .iter()
        .map(|chosen| combined_row(battery, planner, chosen, base, range))
        .collect()
}

/// The candidate sets of [`combined_rows`].
fn combined_builds<'c>(battery: &Battery<'_>, candidates: &'c [Candidate]) -> Vec<Vec<&'c Candidate>> {
    let ranged: Vec<&Candidate> = candidates.iter().filter(|candidate| candidate.kind != RowKind::Accuracy).collect();
    let accurate: Vec<&Candidate> = candidates.iter().filter(|candidate| candidate.kind != RowKind::Range).collect();

    let longest = pick(battery, &ranged, |candidate| -candidate.profiles[0].profile.max_range.value());
    let tightest = pick(battery, &accurate, |candidate| candidate.profiles[0].profile.ideal_radius_coef);

    let mut builds = Vec::new();
    if longest.len() > 1 {
        builds.push(longest.clone());
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
        builds.push(both);
    }

    builds
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
        Source::SpecialAbility => unreachable!("conditional candidates are left out of builds"),
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

/// The items, modules and consumable range multiplier of the build `chosen`.
struct Recipe {
    items: Vec<Item>,
    selection: ShipUpgradeSelection,
    coefficient: RangeCoefficient,
}

impl Recipe {
    fn of(battery: &Battery<'_>, chosen: &[&Candidate]) -> Self {
        let mut recipe =
            Self { items: Vec::new(), selection: battery.selection.clone(), coefficient: RangeCoefficient::NONE };
        for candidate in chosen {
            match &candidate.source {
                Source::Item(item) => recipe.items.push(*item),
                Source::FireControl(param) => ModuleKind::FireControl.set(&mut recipe.selection, param.clone()),
                Source::RangeConsumable(range) => {
                    recipe.coefficient = RangeCoefficient(recipe.coefficient.0 * range.0);
                }
                Source::SpecialAbility => unreachable!("conditional candidates are left out of builds"),
            }
        }
        recipe
    }
}

fn build_label(chosen: &[&Candidate]) -> String {
    chosen.iter().map(|candidate| candidate.label.as_str()).collect::<Vec<_>>().join(" + ")
}

fn combined_row(
    battery: &Battery<'_>,
    planner: &Planner<'_>,
    chosen: &[&Candidate],
    base: &[MountProfile],
    range: Km,
) -> Result<DispersionRow, QueryError> {
    let recipe = Recipe::of(battery, chosen);
    let bundle = planner.modifiers_with(&recipe.items)?;
    let profiles = battery
        .profiles(&bundle, &recipe.selection, recipe.coefficient)
        .expect("the battery has profiles on every selection its candidates came from");
    let kind = change_kind(&base[0], &profiles[0])
        .expect("a build of candidates that each lengthen range or tighten accuracy changes one of them");

    Ok(row(build_label(chosen), kind, &profiles, Some(base), range))
}

fn row(
    build: String,
    kind: RowKind,
    profiles: &[MountProfile],
    base: Option<&[MountProfile]>,
    range: Km,
) -> DispersionRow {
    let max_range = profiles[0].profile.max_range;
    let sigma = profiles[0].sigma;
    let mounts = profiles
        .iter()
        .enumerate()
        .map(|(index, mount)| {
            let base_axes = base.and_then(|base| axes_at(&base[index].profile, range));
            let at_range = match axes_at(&mount.profile, range) {
                Some(axes) => Reach::Ellipse(Ellipse {
                    evaluated_at_km: (axes.at != range).then(|| Km::new(round_to(axes.at.value(), 100.0))),
                    horizontal_m: Meters::new(round_to(axes.horizontal.value(), 10.0)),
                    vertical_m: Meters::new(round_to(axes.vertical.value(), 10.0)),
                    vertical_ratio: VerticalRatio(round_to(axes.ratio.0, 1000.0)),
                    horizontal_change_pct: base_axes
                        .map(|base| change(base.horizontal.value(), axes.horizontal.value())),
                    vertical_change_pct: base_axes.map(|base| change(base.vertical.value(), axes.vertical.value())),
                    half_of_shells_within: sigma.map(|sigma| inner_ellipse(&axes, sigma.fraction_holding(0.5))),
                    ninety_pct_of_shells_within: sigma.map(|sigma| inner_ellipse(&axes, sigma.fraction_holding(0.9))),
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
        shells: sigma.map(ShellSpread::from_sigma),
        mounts,
    }
}

fn inner_ellipse(axes: &Axes, fraction: RadiusFraction) -> InnerEllipse {
    InnerEllipse {
        horizontal_m: Meters::new(round_to(axes.horizontal.value() * fraction.0, 10.0)),
        vertical_m: Meters::new(round_to(axes.vertical.value() * fraction.0, 10.0)),
    }
}

/// The error function, Abramowitz and Stegun 7.1.26 (absolute error below
/// 1.5e-7).
fn erf(x: f64) -> f64 {
    let sign = x.signum();
    let x = x.abs();
    let t = 1.0 / (1.0 + 0.327_591_1 * x);
    let poly =
        t * (0.254_829_592 + t * (-0.284_496_736 + t * (1.421_413_741 + t * (-1.453_152_027 + t * 1.061_405_429))));

    sign * (1.0 - poly * (-x * x).exp())
}

/// The ellipse at `range`. A range slightly past the max range, by up to 5%
/// of it or 1.5 km, whichever is larger, takes the ellipse at the max range:
/// players round distances. `None` farther out.
fn axes_at(profile: &ArtilleryDispersion, range: Km) -> Option<Axes> {
    /// Slack as a share of the max range, and its floor in km.
    const SLACK_SHARE: f32 = 0.05;
    const SLACK_FLOOR_KM: f32 = 1.5;

    let max_range = profile.max_range.value();
    let at = if range.value() <= max_range {
        range
    } else if range.value() <= max_range + (max_range * SLACK_SHARE).max(SLACK_FLOOR_KM) {
        profile.max_range
    } else {
        return None;
    };

    let ellipse = profile.ellipse_at(at);
    Some(Axes {
        at,
        horizontal: ellipse.horizontal.to_meters(),
        vertical: ellipse.vertical.to_meters(),
        ratio: VerticalRatio::at(&profile.curve, at, profile.max_range),
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

/// " With <build> (<range> km range): <h> m horizontal and <v> m vertical."
fn combined_sentence(combined: &DispersionRow) -> String {
    let max = format_km(combined.max_range_km.value());
    match first_ellipse(combined) {
        Some(ellipse) => format!(
            " With {} ({max} km range): {} m horizontal and {} m vertical.",
            combined.build,
            format_m(ellipse.horizontal_m),
            format_m(ellipse.vertical_m)
        ),
        None => format!(" With {} ({max} km range): out of range.", combined.build),
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
            Some((row, Some(ellipse))) => {
                let used = ellipse.evaluated_at_km.map_or_else(
                    || format!("at {at} km"),
                    |used| format!("close enough to take at {} km", format_km(used.value())),
                );
                text.push_str(&format!(
                    " With {} it reaches {} km; {used}, dispersion is {} m horizontal and {} m vertical.",
                    row.build,
                    format_km(row.max_range_km.value()),
                    format_m(ellipse.horizontal_m),
                    format_m(ellipse.vertical_m),
                ));
            }
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

    if let (Some(shells), Some(half)) = (&base.shells, base_ellipse.half_of_shells_within.as_ref()) {
        text.push_str(&format!(
            " With sigma {}, half of the shells land within {}% of the ellipse ({} m by {} m) and {}% within its inner half; a higher sigma packs more shells toward the aim point.",
            shells.sigma_count.0,
            round_to(shells.half_of_shells_radius_fraction.0 * 100.0, 10.0),
            format_m(half.horizontal_m),
            format_m(half.vertical_m),
            round_to(shells.inner_half_share_pct.value(), 10.0),
        ));
    }

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

    if let Some(used) = base_ellipse.evaluated_at_km {
        text = format!(
            "{at} km is past {}'s {} km {subject} range; base values use {} km. {text}",
            ship.name,
            format_km(base.max_range_km.value()),
            format_km(used.value()),
        );
    }

    text
}

/// The build a ship is ranked on in a group: the shortest range build that
/// reaches the distance, else the longest one whose range falls slightly
/// short. Conditional and accuracy rows are left out.
fn ranked_row(rows: &[DispersionRow]) -> Option<&DispersionRow> {
    let usable =
        || rows.iter().filter(|row| row.condition.is_none() && matches!(row.kind, RowKind::Base | RowKind::Range));
    let by_range = |left: &&DispersionRow, right: &&DispersionRow| {
        left.max_range_km.value().total_cmp(&right.max_range_km.value())
    };

    usable()
        .filter(|row| first_ellipse(row).is_some_and(|ellipse| ellipse.evaluated_at_km.is_none()))
        .min_by(by_range)
        .or_else(|| usable().filter(|row| first_ellipse(row).is_some()).max_by(by_range))
}

/// Rank of the first entry (1 is the smallest value), the median and the
/// extremes. `None` when the first entry has no value.
fn standing(entries: &[(String, GroupEntry)], value: impl Fn(&GroupEntry) -> Option<f32>) -> Option<Standing> {
    let own = value(&entries[0].1)?;
    let mut valued: Vec<(&str, &str, f32)> = entries
        .iter()
        .filter_map(|(ship, entry)| value(entry).map(|value| (ship.as_str(), entry.build.as_str(), value)))
        .collect();
    valued.sort_by(|left, right| left.2.total_cmp(&right.2));

    let middle = valued.len() / 2;
    let median =
        if valued.len().is_multiple_of(2) { (valued[middle - 1].2 + valued[middle].2) / 2.0 } else { valued[middle].2 };
    let named = |(ship, build, value): (&str, &str, f32)| GroupShip {
        ship: ship.to_owned(),
        build: build.to_owned(),
        value_m: Meters::new(round_to(value, 10.0)),
    };

    Some(Standing {
        rank: 1 + valued.iter().filter(|(_, _, other)| *other < own).count(),
        value_m: Meters::new(round_to(own, 10.0)),
        median_m: Meters::new(round_to(median, 10.0)),
        tightest: named(valued[0]),
        widest: named(valued[valued.len() - 1]),
    })
}

/// A direct comparison between the base build and the build asked about.
fn build_comparison_sentence(
    ship: &Ship,
    kind: DispersionBattery,
    range: Km,
    base: &DispersionRow,
    selected: &DispersionRow,
) -> String {
    let at = format_km(range.value());
    let selected_max = format_km(selected.max_range_km.value());
    let Some(selected_ellipse) = first_ellipse(selected) else {
        return format!(
            "At {at} km, {}'s {} is out of range both without and with {}; its range with {} is {selected_max} km.",
            ship.name,
            kind.noun(),
            selected.build,
            selected.build,
        );
    };
    let used = selected_ellipse.evaluated_at_km.unwrap_or(range);
    let distance = format_km(used.value());
    let Some(base_ellipse) = first_ellipse(base) else {
        return format!(
            "Without {}, {}'s {} reaches {} km, short of {at} km. With {} ({selected_max} km range), dispersion at {distance} km is {} m horizontal and {} m vertical.",
            selected.build,
            ship.name,
            kind.noun(),
            format_km(base.max_range_km.value()),
            selected.build,
            format_m(selected_ellipse.horizontal_m),
            format_m(selected_ellipse.vertical_m),
        );
    };
    let base_used = base_ellipse.evaluated_at_km.unwrap_or(range);
    if base_used != used {
        return format!(
            "At the requested {at} km, {}'s {} builds are evaluated at different distances: without {}, {} m horizontal and {} m vertical at {} km; with {}, {} m horizontal and {} m vertical at {distance} km.",
            ship.name,
            kind.noun(),
            selected.build,
            format_m(base_ellipse.horizontal_m),
            format_m(base_ellipse.vertical_m),
            format_km(base_used.value()),
            selected.build,
            format_m(selected_ellipse.horizontal_m),
            format_m(selected_ellipse.vertical_m),
        );
    }

    let clamped = selected_ellipse
        .evaluated_at_km
        .map(|_| format!(" {at} km is past its {selected_max} km range, so the comparison uses {distance} km."))
        .unwrap_or_default();
    format!(
        "At {distance} km, {}'s {} dispersion without {} is {} m horizontal and {} m vertical; with {} it is {} m horizontal and {} m vertical ({} horizontal, {} vertical).{clamped}",
        ship.name,
        kind.noun(),
        selected.build,
        format_m(base_ellipse.horizontal_m),
        format_m(base_ellipse.vertical_m),
        selected.build,
        format_m(selected_ellipse.horizontal_m),
        format_m(selected_ellipse.vertical_m),
        format_pct(selected_ellipse.horizontal_change_pct.unwrap_or(Percent::new(0.0))),
        format_pct(selected_ellipse.vertical_change_pct.unwrap_or(Percent::new(0.0))),
    )
}

fn group_sentence(ship: &Ship, range: Km, group: &GroupComparison) -> String {
    let tiers = if group.min_tier == group.max_tier {
        format!("tier {}", group.min_tier.0)
    } else {
        format!("tier {}-{}", group.min_tier.0, group.max_tier.0)
    };
    let half = group
        .half_of_shells_horizontal
        .as_ref()
        .map_or_else(String::new, |half| format!(" and {} on the radius holding half of the shells", half.rank));
    format!(
        " Among {tiers} {}s at {} km ({} ranked, {} out of reach), {} on {} ranks {} of {} on horizontal dispersion, {} on vertical{half}; the median is {} m horizontal and {} m vertical, the tightest horizontal is {} ({} m).",
        group.class.label().replace('_', " "),
        format_km(range.value()),
        group.ships_ranked,
        group.ships_out_of_reach,
        ship.name,
        group.ship_build,
        group.horizontal.rank,
        group.ships_ranked,
        group.vertical.rank,
        format_m(group.horizontal.median_m),
        format_m(group.vertical.median_m),
        group.horizontal.tightest.ship,
        format_m(group.horizontal.tightest.value_m),
    )
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
        // Within 1.5 km past max range the ellipse is taken at the max range.
        let clamped = axes_at(&short, Km::new(21.0)).expect("21 km is within the slack");
        assert_eq!(clamped.at, Km::new(20.0));
        assert_eq!(axes_at(&short, Km::new(21.6)), None);
    }

    fn mount(max_range_km: f32, ideal_radius_coef: f32, sigma: f32) -> MountProfile {
        MountProfile {
            mount: "main battery".to_owned(),
            profile: ArtilleryDispersion { curve: curve(), max_range: Km::new(max_range_km), ideal_radius_coef },
            sigma: Some(SigmaCount(sigma)),
        }
    }

    #[test]
    fn change_kind_tells_range_from_accuracy() {
        let base = mount(20.0, 1.0, 2.0);

        assert_eq!(change_kind(&base, &mount(20.0, 1.0, 2.0)), None);
        assert_eq!(change_kind(&base, &mount(23.2, 1.0, 2.0)), Some(RowKind::Range));
        assert_eq!(change_kind(&base, &mount(20.0, 0.93, 2.0)), Some(RowKind::Accuracy));
        assert_eq!(change_kind(&base, &mount(20.0, 1.0, 2.1)), Some(RowKind::Accuracy));
        assert_eq!(change_kind(&base, &mount(23.2, 0.93, 2.0)), Some(RowKind::RangeAndAccuracy));
    }

    #[test]
    fn shell_share_follows_the_clamped_normal() {
        let sigma = SigmaCount(2.0);
        let share = |t: f32| sigma.share_within(RadiusFraction(t));

        assert!(share(0.0).abs() < 1e-9);
        assert!((share(1.0) - 1.0).abs() < 1e-6);
        // erf(0.5 x 2 / sqrt 2) = 0.682689; the 4.55% redrawn uniformly add half of themselves.
        assert!((share(0.5) - (0.682_689 + 0.045_500 * 0.5)).abs() < 1e-5, "got {}", share(0.5));
        // sigma 1: erf(0.353553) = 0.382925 plus half of 1 - erf(0.707107) = 0.317311.
        assert!((SigmaCount(1.0).share_within(RadiusFraction(0.5)) - (0.382_925 + 0.317_311 * 0.5)).abs() < 1e-5);
    }

    #[test]
    fn radius_fraction_inverts_the_share() {
        for sigma in [1.0, 1.9, 2.0, 2.1, 2.8] {
            let sigma = SigmaCount(sigma);
            for share in [0.5, 0.9] {
                let fraction = sigma.fraction_holding(share);
                assert!((sigma.share_within(fraction) - share).abs() < 1e-5);
            }
        }
        // Higher sigma holds half of the shells closer to the aim point.
        assert!(SigmaCount(2.1).fraction_holding(0.5).0 < SigmaCount(1.9).fraction_holding(0.5).0);
    }
}
