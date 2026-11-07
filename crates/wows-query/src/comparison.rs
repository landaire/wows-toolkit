//! Ships of one class and tier range compared on the factors that decide
//! their strength, each ranked within the group, with labels for clear
//! outliers, per armament and per torpedo option. No composite score: the
//! factors trade against each other differently by play style.

use std::collections::HashMap;

use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use serde::ser::SerializeMap;
use wows_core::units::Degrees;
use wows_core::units::Km;
use wows_ship_stats::Bearing;
use wows_ship_stats::bearing::mount_bears;
use wows_ship_stats::mount_salvo;
use wows_ship_stats::mounts::aiming_mounts;
use wowsunpack::ballistics::ShellParams;
use wowsunpack::ballistics::solve_for_range;
use wowsunpack::game_params::keys::ComponentType;
use wowsunpack::game_params::ttx::constants::BW_KNOTS_TO_MPS;
use wowsunpack::game_params::ttx::constants::BW_TO_BALLISTIC;
use wowsunpack::game_params::ttx::constants::KM_TO_M;
use wowsunpack::game_params::ttx::model::ShellStats;
use wowsunpack::game_params::ttx::model::TorpedoStats;
use wowsunpack::game_params::ttx::modifiers::ModifierBundle;
use wowsunpack::game_params::ttx::orchestration::artillery_sigma;
use wowsunpack::game_params::ttx::selection::ShipUpgradeSelection;
use wowsunpack::game_params::types::GameParamProvider;
use wowsunpack::game_params::types::MountPoint;

use crate::armor_assessment::class_plural;
use crate::armor_assessment::parallel_map;
use crate::armor_trace::largest_shell;
use crate::builds::Better;
use crate::builds::Charges;
use crate::builds::Outcome;
use crate::builds::Planner;
use crate::builds::StatAmount;
use crate::builds::StatPlan;
use crate::consumables::consumable_stats;
use crate::data::GameData;
use crate::error::QueryError;
use crate::error::RawParamsError;
use crate::names::Interpretations;
use crate::shells::Ammo;
use crate::shells::Battery;
use crate::shells::Shell;
use crate::ship_variants::listing_order;
use crate::ships::Ship;
use crate::ships::ShipClass;
use crate::ships::ShipFilter;
use crate::ships::ShipStatus;
use crate::ships::Tier;
use crate::ships::TierRange;
use crate::special_abilities::SpecialAbility;
use crate::stats::extended_horizontal_dispersion;

/// Ships besides the one labelled a group must hold before a value is
/// labelled an outlier: in smaller groups the extremes say little.
const OUTLIER_MIN_OTHERS: usize = 4;

/// Share of the other ships a value must beat, or lose to, to be labelled
/// an outlier.
const OUTLIER_SHARE: f32 = 0.9;

/// Ballistic meters per second per knot of torpedo speed: BigWorld units
/// per second per knot times ballistic meters per BigWorld unit.
const TORPEDO_METERS_PER_SECOND_PER_KNOT: f32 = BW_KNOTS_TO_MPS * BW_TO_BALLISTIC;

/// Arguments for `ship_comparison`.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ShipComparisonArgs {
    /// Class compared. Optional with ships: then the class of the first ship named.
    pub class: Option<ShipClass>,

    /// One tier, e.g. 10. Use min_tier and max_tier for a range, e.g. 9 to 11 for a matchmaking band or 1 to 11 for every tier of the class.
    pub tier: Option<Tier>,
    pub min_tier: Option<Tier>,
    pub max_tier: Option<Tier>,

    /// Nation as GameParams spells it, e.g. "USA".
    pub nation: Option<String>,

    /// Ships to list, e.g. ["Incomparable", "C. Colombo"]. Ranks still compare each with the whole group, which also takes in every ship named. Without tiers, the group spans the tiers of the ships named.
    pub ships: Option<Vec<String>>,

    /// Range in km at which dispersion and shell flight time are compared. Defaults to 15 for battleships, 12 for cruisers and 10 for other classes.
    pub reference_range_km: Option<Km>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ShipComparisonOutput {
    /// Names read as something they do not spell exactly, or as one of several ships sharing a name.
    #[serde(skip_serializing_if = "Interpretations::is_empty")]
    pub interpretations: Interpretations,

    /// e.g. "tier 10 battleships".
    pub group: String,

    /// Ships ranked: released ships and ships in testing, premium replicas
    /// named in their original's entry.
    pub ranked: usize,

    /// Ships in testing among `ranked`.
    #[serde(skip_serializing_if = "is_zero")]
    pub test: usize,

    /// Torpedo options of the ranked ships, which torpedo factors rank
    /// among.
    #[serde(skip_serializing_if = "is_zero")]
    pub torpedo_options: usize,

    pub reference_range_km: Km,

    /// "key (unit, higher or lower is better): what it measures", for each
    /// factor some ship of the group has.
    pub factors: Vec<String>,

    /// The same for the factors of each torpedo option.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub torpedo_factors: Vec<String>,

    pub ships: Vec<ComparedShip>,
    pub notes: Vec<&'static str>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ComparedShip {
    /// "T10 C. Colombo (battleship, Italy)", ending ", test" for a ship in testing.
    pub ship: String,

    /// Premium replicas of this ship, left out of the ranks.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub replicas: Vec<String>,

    /// Per factor, "value (rank r/n, pNN)".
    pub values: FactorValues,

    /// Values that beat, or lose to, at least 90% of the other ships, or
    /// torpedo options, in groups of five or more.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub outliers: Vec<String>,

    /// Each torpedo the ship's torpedo modules carry.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub torpedoes: Vec<TorpedoOption>,

    /// The ship's unique upgrade and the factors whose best-fit build takes it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unique_upgrade: Option<String>,

    /// Stock consumables by slot, e.g. "Repair Party (4 charges, 80 s reload)";
    /// options of one slot are joined with " or ".
    pub consumables: Vec<String>,

    /// The ship's special abilities (the F key) with their effects.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub special_abilities: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TorpedoOption {
    /// English name.
    pub torpedo: String,

    /// Modules other than stock that carry it. Empty for the stock torpedo.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub modules: Vec<String>,

    /// Per torpedo factor, "value (rank r/n, pNN)" among every torpedo option
    /// of the group.
    pub values: FactorValues,

    /// For a torpedo whose damage depends on the distance it has run, the
    /// share of full damage by distance.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub damage_by_distance: Option<String>,

    /// Factors whose best-fit build changes the value, with the build and
    /// the value without it, e.g. "speed 82.69 kn with Torpedo Acceleration;
    /// 75 kn without".
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub builds: Vec<String>,
}

/// Values in order, serialized as a map from key to value.
#[derive(Debug, Clone, Default)]
pub struct FactorValues(Vec<(&'static str, String)>);

impl Serialize for FactorValues {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (key, value) in &self.0 {
            map.serialize_entry(key, value)?;
        }
        map.end()
    }
}

/// What a factor measures and which way it improves.
struct Meta {
    key: &'static str,
    unit: &'static str,
    better: Better,
    description: &'static str,
}

impl Meta {
    fn legend(&self) -> String {
        let better = match self.better {
            Better::Lower => "lower",
            Better::Higher => "higher",
        };
        format!("{} ({}, {better} is better): {}", self.key, self.unit, self.description)
    }

    /// `value` with its unit, e.g. "2.1 km" or "35%".
    fn with_unit(&self, value: f32) -> String {
        match self.unit {
            "%" => format!("{}%", number(value)),
            "x" => format!("{}x", number(value)),
            unit => format!("{} {unit}", number(value)),
        }
    }
}

/// One compared factor of a ship.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum Factor {
    Health,
    RepairPartyHeal,
    MainBatteryReload,
    MainBatteryRange,
    DamagePerMinute(AmmoKind),
    Alpha(AmmoKind),
    FlightTime(AmmoKind),
    TurretTraverse,
    Dispersion,
    Sigma,
    FiresPerMinute,
    BowBarrels,
    SternBarrels,
    AllBearFromBow,
    AllBearFromStern,
    SecondaryDamagePerMinute,
    SecondaryRange,
    TorpedoDamagePerMinute,
    TorpedoesPerMinute,
    TorpedoReload,
    TorpedoLaunchFromBow,
    BroadsideTubes,
    AntiAircraftDamage,
    AntiAircraftRange,
    Concealment,
    AirConcealment,
    Speed,
    RudderShift,
    TurningCircle,
}

impl Factor {
    fn all() -> Vec<Self> {
        let mut factors = vec![Self::Health, Self::RepairPartyHeal, Self::MainBatteryReload, Self::MainBatteryRange];
        factors.extend(AmmoKind::ALL.map(Self::DamagePerMinute));
        factors.extend(AmmoKind::ALL.map(Self::Alpha));
        factors.extend(AmmoKind::ALL.map(Self::FlightTime));
        factors.extend([
            Self::TurretTraverse,
            Self::Dispersion,
            Self::Sigma,
            Self::FiresPerMinute,
            Self::BowBarrels,
            Self::SternBarrels,
            Self::AllBearFromBow,
            Self::AllBearFromStern,
            Self::SecondaryDamagePerMinute,
            Self::SecondaryRange,
            Self::TorpedoDamagePerMinute,
            Self::TorpedoesPerMinute,
            Self::TorpedoReload,
            Self::TorpedoLaunchFromBow,
            Self::BroadsideTubes,
            Self::AntiAircraftDamage,
            Self::AntiAircraftRange,
            Self::Concealment,
            Self::AirConcealment,
            Self::Speed,
            Self::RudderShift,
            Self::TurningCircle,
        ]);

        factors
    }

    fn meta(self) -> Meta {
        let (key, unit, better, description) = match self {
            Self::Health => ("health", "HP", Better::Higher, "hit points"),
            Self::RepairPartyHeal => (
                "repair_party_heal",
                "%",
                Better::Higher,
                "share of maximum HP one Repair Party charge restores, stock",
            ),
            Self::MainBatteryReload => ("main_battery_reload", "s", Better::Lower, "main battery reload"),
            Self::MainBatteryRange => ("main_battery_range", "km", Better::Higher, "main battery firing range"),
            Self::DamagePerMinute(ammo) => (
                ammo.key("dpm"),
                "HP/min",
                Better::Higher,
                "main battery damage per minute of this ammo against one target at the bearing where the most of it bears, every shell hitting",
            ),
            Self::Alpha(ammo) => (
                ammo.key("alpha"),
                "HP",
                Better::Higher,
                "damage of one salvo of every main battery barrel of this ammo",
            ),
            Self::FlightTime(ammo) => (
                ammo.key("flight_time"),
                "s",
                Better::Lower,
                "flight time of the largest main battery shell of this ammo to the reference range; absent when the range is beyond the ship's reach",
            ),
            Self::TurretTraverse => {
                ("turret_traverse", "s", Better::Lower, "time a main battery turret takes to turn 180 deg")
            }
            Self::Dispersion => (
                "dispersion",
                "m",
                Better::Lower,
                "main battery horizontal dispersion at the reference range, or at the ship's own range when shorter",
            ),
            Self::Sigma => {
                ("sigma", "sigma", Better::Higher, "main battery sigma; higher puts more shells near the aim point")
            }
            Self::FiresPerMinute => (
                "fires_per_minute",
                "fires/min",
                Better::Higher,
                "fire starts per minute of every armament against one target at its best bearing, every projectile hitting",
            ),
            Self::BowBarrels => (
                "bow_barrels",
                "%",
                Better::Higher,
                "share of main battery barrels that bear on a target within 30 deg of the bow, at the best bearing",
            ),
            Self::SternBarrels => (
                "stern_barrels",
                "%",
                Better::Higher,
                "share of main battery barrels that bear on a target within 30 deg of the stern, at the best bearing; kiting",
            ),
            Self::AllBearFromBow => (
                "all_bear_from_bow",
                "deg",
                Better::Lower,
                "smallest angle off the bow at which every main battery barrel bears",
            ),
            Self::AllBearFromStern => (
                "all_bear_from_stern",
                "deg",
                Better::Lower,
                "smallest angle off the stern at which every main battery barrel bears; the kiting angle",
            ),
            Self::SecondaryDamagePerMinute => (
                "secondary_dpm",
                "HP/min",
                Better::Higher,
                "secondary battery damage per minute, each group at its own best bearing, added up, every shell hitting",
            ),
            Self::SecondaryRange => ("secondary_range", "km", Better::Higher, "secondary battery firing range"),
            Self::TorpedoDamagePerMinute => (
                "torpedo_dpm",
                "HP/min",
                Better::Higher,
                "torpedo damage per minute of the best torpedo against one target at its best bearing, every torpedo hitting",
            ),
            Self::TorpedoesPerMinute => (
                "torpedoes_per_minute",
                "torpedoes/min",
                Better::Higher,
                "torpedoes launched per minute against one target at its best bearing, with the loaders' limit",
            ),
            Self::TorpedoReload => ("torpedo_reload", "s", Better::Lower, "torpedo tube reload"),
            Self::TorpedoLaunchFromBow => (
                "torpedo_launch_from_bow",
                "deg",
                Better::Lower,
                "smallest angle off the bow at which a torpedo launcher bears; 0 launches bow-on",
            ),
            Self::BroadsideTubes => {
                ("broadside_tubes", "tubes", Better::Higher, "torpedo tubes that bear on one broadside at once")
            }
            Self::AntiAircraftDamage => (
                "aa_damage",
                "HP/s",
                Better::Higher,
                "continuous anti-aircraft damage per second of every aura, stock, before hit chance; flak bursts not counted",
            ),
            Self::AntiAircraftRange => {
                ("aa_range", "km", Better::Higher, "reach of the longest anti-aircraft aura, stock")
            }
            Self::Concealment => ("concealment", "km", Better::Lower, "detectability by ships"),
            Self::AirConcealment => ("air_concealment", "km", Better::Lower, "detectability by aircraft"),
            Self::Speed => ("speed", "kn", Better::Higher, "top speed"),
            Self::RudderShift => ("rudder_shift", "s", Better::Lower, "rudder shift time"),
            Self::TurningCircle => ("turning_circle", "m", Better::Lower, "turning circle radius"),
        };

        Meta { key, unit, better, description }
    }

    /// `value` to the precision it is compared at: whole hit points, meters
    /// and degrees, else two decimals.
    fn rounded(self, value: f32) -> f32 {
        match self {
            Self::Health
            | Self::Alpha(_)
            | Self::DamagePerMinute(_)
            | Self::SecondaryDamagePerMinute
            | Self::TorpedoDamagePerMinute
            | Self::AntiAircraftDamage
            | Self::Dispersion
            | Self::TurningCircle => value.round(),
            _ => round2(value),
        }
    }
}

/// One compared factor of a torpedo option.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum TorpedoFactor {
    Speed,
    Range,
    Damage,
    DamageAtMaxRange,
    FloodCoefficient,
    Detectability,
    ReactionTime,
    DamagePerMinute,
}

impl TorpedoFactor {
    const ALL: [Self; 8] = [
        Self::Speed,
        Self::Range,
        Self::Damage,
        Self::DamageAtMaxRange,
        Self::FloodCoefficient,
        Self::Detectability,
        Self::ReactionTime,
        Self::DamagePerMinute,
    ];

    fn meta(self) -> Meta {
        let (key, unit, better, description) = match self {
            Self::Speed => ("speed", "kn", Better::Higher, "torpedo speed"),
            Self::Range => ("range", "km", Better::Higher, "torpedo range"),
            Self::Damage => ("damage", "HP", Better::Higher, "damage of one torpedo hit at full damage"),
            Self::DamageAtMaxRange => (
                "damage_at_max_range",
                "HP",
                Better::Higher,
                "damage of one torpedo hit at the end of its run; less than damage for torpedoes whose damage falls with distance",
            ),
            Self::FloodCoefficient => (
                "flood_coefficient",
                "x",
                Better::Higher,
                "flooding coefficient of one hit (uwCritical), reduced by the target's torpedo protection; a relative weight, not a probability",
            ),
            Self::Detectability => ("detectability", "km", Better::Lower, "distance at which ships detect the torpedo"),
            Self::ReactionTime => (
                "reaction_time",
                "s",
                Better::Lower,
                "time a target has from detecting the torpedo until it arrives: detectability / speed",
            ),
            Self::DamagePerMinute => {
                ("damage_per_minute", "HP/min", Better::Higher, "torpedo damage per minute, every torpedo hitting")
            }
        };

        Meta { key, unit, better, description }
    }

    fn rounded(self, value: f32) -> f32 {
        match self {
            Self::Damage | Self::DamageAtMaxRange | Self::DamagePerMinute => value.round(),
            _ => round2(value),
        }
    }
}

/// Main battery ammo, as damage rates name it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum AmmoKind {
    Ap,
    He,
    Sap,
}

impl AmmoKind {
    const ALL: [Self; 3] = [Self::Ap, Self::He, Self::Sap];

    fn from_label(label: &str) -> Option<Self> {
        match label {
            "AP" => Some(Self::Ap),
            "HE" => Some(Self::He),
            "SAP" => Some(Self::Sap),
            _ => None,
        }
    }

    /// The kind of a stat card shell row, whose ammo kind reads "CS" for SAP.
    fn of_shell(shell: &ShellStats) -> Option<Self> {
        match shell.ammo_kind.as_deref()? {
            "AP" => Some(Self::Ap),
            "HE" => Some(Self::He),
            "CS" => Some(Self::Sap),
            _ => None,
        }
    }

    fn ammo(self) -> Ammo {
        match self {
            Self::Ap => Ammo::Ap,
            Self::He => Ammo::He,
            Self::Sap => Ammo::Sap,
        }
    }

    fn key(self, prefix: &'static str) -> &'static str {
        match (prefix, self) {
            ("dpm", Self::Ap) => "dpm_ap",
            ("dpm", Self::He) => "dpm_he",
            ("dpm", Self::Sap) => "dpm_sap",
            ("alpha", Self::Ap) => "alpha_ap",
            ("alpha", Self::He) => "alpha_he",
            ("alpha", Self::Sap) => "alpha_sap",
            (_, Self::Ap) => "flight_time_ap",
            (_, Self::He) => "flight_time_he",
            (_, Self::Sap) => "flight_time_sap",
        }
    }
}

/// A card value of one build, `None` when the ship lacks it.
type CardValue<'f> = &'f dyn Fn(&Outcome) -> Option<f32>;

/// What one ship scores, before ranks.
struct ShipScores {
    values: HashMap<Factor, f32>,

    /// Factors whose best-fit build takes the ship's unique upgrade.
    unique_used: Vec<Factor>,
    unique_upgrade: Option<String>,
    consumables: Vec<String>,
    special_abilities: Vec<String>,
    torpedoes: Vec<TorpedoScores>,
}

struct TorpedoScores {
    name: String,
    modules: Vec<String>,
    values: HashMap<TorpedoFactor, f32>,
    damage_by_distance: Option<String>,
    builds: Vec<String>,
}

/// The stat card values of one torpedo under one build.
#[derive(Debug, Clone, Copy)]
struct TorpedoCard {
    speed: Option<f32>,
    range: Option<f32>,
    damage: Option<f32>,
    visibility: Option<f32>,
}

impl TorpedoCard {
    /// The torpedo of GameParams name `param` on `outcome`'s card; `None`
    /// when the build's modules do not carry it.
    fn of(outcome: &Outcome, param: &str) -> Option<Self> {
        let torpedo: &TorpedoStats =
            outcome.stats.torpedoes.as_ref()?.torpedoes.iter().find(|torpedo| torpedo.name == param)?;

        Some(Self {
            speed: torpedo.speed.map(|speed| speed.value()),
            range: torpedo.range.map(|range| range.value()),
            damage: torpedo.damage.map(|damage| damage.value()),
            visibility: torpedo.visibility.map(|visibility| visibility.value()),
        })
    }
}

/// An aiming mount, its barrels or tubes and the facing of its hardpoint.
struct FacedMount<'a> {
    mount: &'a MountPoint,
    barrels: u32,
    facing: Degrees,
}

/// How a battery's barrels bear on targets ahead and astern.
#[derive(Debug, Clone, Copy)]
struct MountArcs {
    barrels: u32,

    /// Most barrels that bear at one bearing within 30 deg of the bow, and
    /// of the stern.
    bow: u32,
    stern: u32,

    /// Smallest angles off the bow and off the stern at which every barrel
    /// bears; `None` when they never all bear there.
    all_from_bow: Option<u16>,
    all_from_stern: Option<u16>,

    /// Smallest angle off the bow at which any barrel bears.
    any_from_bow: Option<u16>,

    /// Most barrels that bear at one bearing on either broadside.
    broadside: u32,
}

impl MountArcs {
    /// Arcs as ship_firing_arcs names them: bow 331-29, stern 151-209,
    /// broadsides 30-150 and 210-330.
    fn of(mounts: &[FacedMount<'_>]) -> Option<Self> {
        let barrels: u32 = mounts.iter().map(|mount| mount.barrels).sum();
        if barrels == 0 {
            return None;
        }

        let bearing: Vec<u32> = Bearing::all()
            .map(|bearing| {
                mounts
                    .iter()
                    .filter(|mount| mount_bears(mount.mount, mount.facing, bearing))
                    .map(|mount| mount.barrels)
                    .sum()
            })
            .collect();
        let most = |degrees: &mut dyn Iterator<Item = u16>| -> u32 {
            degrees.map(|degrees| bearing[usize::from(degrees)]).max().unwrap_or(0)
        };
        let from_bow = |degrees: u16| degrees.min(360 - degrees);
        let from_stern = |degrees: u16| degrees.abs_diff(180);
        let all = (0..360u16).filter(|degrees| bearing[usize::from(*degrees)] == barrels);

        Some(Self {
            barrels,
            bow: most(&mut (331..360).chain(0..=29)),
            stern: most(&mut (151..=209)),
            all_from_bow: all.clone().map(from_bow).min(),
            all_from_stern: all.map(from_stern).min(),
            any_from_bow: (0..360u16).filter(|degrees| bearing[usize::from(*degrees)] > 0).map(from_bow).min(),
            broadside: most(&mut (30..=150).chain(210..=330)),
        })
    }

    fn share(&self, barrels: u32) -> f32 {
        barrels as f32 * 100.0 / self.barrels as f32
    }
}

/// A ship's rank among the values of a group, the ship's own included.
struct Standing {
    count: usize,

    /// Other values better and worse than the ship's.
    better: usize,
    worse: usize,
}

impl Standing {
    fn new(value: f32, better: Better, values: impl Iterator<Item = f32>) -> Self {
        let mut standing = Self { count: 0, better: 0, worse: 0 };
        for other in values {
            standing.count += 1;
            if better.improves(other, value) {
                standing.better += 1;
            } else if better.improves(value, other) {
                standing.worse += 1;
            }
        }

        standing
    }

    fn others(&self) -> usize {
        self.count.saturating_sub(1)
    }

    /// "value (rank r/n, pNN)": pNN is the share of the other values the
    /// ship's beats.
    fn ranked(&self, shown: &str, scope: &str) -> String {
        match self.others() {
            0 => format!("{shown} (the only one of the {scope} with it)"),
            others => {
                let percentile = (self.worse as f32 * 100.0 / others as f32).round();
                format!("{shown} (rank {}/{}, p{percentile})", 1 + self.better, self.count)
            }
        }
    }

    /// "worse than 95% of the 40 other tier 10 destroyers (rank 41/41)" when
    /// the value is an outlier.
    fn outlier(&self, scope: &str) -> Option<String> {
        let others = self.others();
        if others < OUTLIER_MIN_OTHERS {
            return None;
        }

        let (word, beyond) = if self.better as f32 >= OUTLIER_SHARE * others as f32 {
            ("worse", self.better)
        } else if self.worse as f32 >= OUTLIER_SHARE * others as f32 {
            ("better", self.worse)
        } else {
            return None;
        };
        let than = if beyond == others {
            format!("all {others} other {scope}")
        } else {
            format!("{}% of the {others} other {scope}", (beyond as f32 * 100.0 / others as f32).round())
        };

        Some(format!("{word} than {than} (rank {}/{})", 1 + self.better, self.count))
    }
}

impl GameData {
    /// The ships of one class and tier range with each factor of strength
    /// and its rank in the group.
    pub fn ship_comparison(&self, args: &ShipComparisonArgs) -> Result<ShipComparisonOutput, QueryError> {
        const NOTES: &[&str] = &[
            "Values are on each ship's best-fit build for that factor, as ship_stats reports them, with the ship's unique upgrade where it improves the factor; unique_upgrade names the factors that take it. Firing arcs use the module combination with the most barrels or tubes. Consumables, repair_party_heal, anti-aircraft and torpedo flood_coefficient are stock.",
            "rank r/n: position among the n ships of the group with the factor, 1 best, ties sharing a rank. pNN: the share of the other n - 1 ships the ship beats. Torpedo factors rank among every torpedo option of the group's ships.",
            "outliers: values better or worse than at least 90% of the others, only where there are at least 4 others.",
            "Nominal plate thickness is not a factor: protection depends on every plate in front of the citadel.",
            "Torpedo reaction_time = detectability / speed, with speed in knots x 2.69 m/s per knot (the client's BW_KNOTS_TO_MPS times 30 m per BigWorld unit).",
            "Damage rates assume every shell or torpedo hits; dispersion, sigma, range, flight time and concealment decide how much of it lands. No composite score is given: the factors trade against each other by play style.",
        ];

        let mut interpretations = Interpretations::default();
        let mut named: Vec<&Ship> = Vec::new();
        for name in args.ships.iter().flatten() {
            let ship = self.resolve_ship(name, &mut interpretations)?;
            // A replica is ranked as its original, which names it.
            let ship = match &ship.replica_of {
                Some(original) => self.ships.iter().find(|other| other.param.index() == original.index).unwrap_or(ship),
                None => ship,
            };
            if !named.iter().any(|other| other.param.index() == ship.param.index()) {
                named.push(ship);
            }
        }

        let Some(class) = args.class.or_else(|| named.first().map(|ship| ship.class)) else {
            return Err(QueryError::InvalidArguments { reason: "give class, or name ships" });
        };
        if named.iter().any(|ship| ship.class != class) {
            return Err(QueryError::InvalidArguments { reason: "every ship named must be of the class compared" });
        }
        let tiers = match (args.tier, TierRange::from_bounds(args.min_tier, args.max_tier)) {
            (Some(tier), _) => TierRange { min: tier, max: tier },
            (None, Some(range)) => range,
            (None, None) => {
                match (named.iter().map(|ship| ship.tier).min(), named.iter().map(|ship| ship.tier).max()) {
                    (Some(min), Some(max)) => TierRange { min, max },
                    _ => {
                        return Err(QueryError::InvalidArguments {
                            reason: "give tier, min_tier or max_tier, or name ships",
                        });
                    }
                }
            }
        };
        if tiers.min > tiers.max {
            return Err(QueryError::InvalidArguments { reason: "min_tier must not exceed max_tier" });
        }

        let filter = ShipFilter {
            tiers: Some(tiers),
            class: Some(class),
            nation: args.nation.clone(),
            include_irregular: false,
            exclude_test: false,
            include_replicas: false,
        };
        let mut group: Vec<&Ship> = self.ships.iter().filter(|ship| filter.matches(ship)).collect();
        for ship in &named {
            if !group.iter().any(|other| other.param.index() == ship.param.index()) {
                group.push(ship);
            }
        }
        group.sort_by(|left, right| listing_order(left, right));

        let reference_range = args.reference_range_km.unwrap_or(match class {
            ShipClass::Battleship => Km::new(15.0),
            ShipClass::Cruiser => Km::new(12.0),
            ShipClass::Destroyer | ShipClass::AircraftCarrier | ShipClass::Submarine => Km::new(10.0),
        });
        if reference_range.value().is_nan() || reference_range.value() <= 0.0 {
            return Err(QueryError::InvalidArguments { reason: "reference_range_km must be more than 0" });
        }

        // Anti-aircraft auras are read from raw GameParams, which need a
        // cache directory.
        let anti_aircraft = match self.raw_params() {
            Ok(_) => true,
            Err(QueryError::RawParams { source }) if matches!(*source, RawParamsError::NoCacheDir) => false,
            Err(error) => return Err(error),
        };
        let scores: Vec<ShipScores> =
            parallel_map(&group, |ship| self.ship_scores(ship, reference_range, anti_aircraft))
                .into_iter()
                .collect::<Result<_, _>>()?;

        // The group's tiers as ranked, which ships named may widen.
        let shown_tiers = match (group.iter().map(|ship| ship.tier).min(), group.iter().map(|ship| ship.tier).max()) {
            (Some(min), Some(max)) => TierRange { min, max },
            _ => tiers,
        };
        let nation = args.nation.as_deref().map(|nation| format!("{nation} ")).unwrap_or_default();
        let noun = format!("{nation}{}", class_plural(class));
        let group_label = format!("{} {noun}", tier_label(shown_tiers));
        let torpedo_scope = format!("torpedo options of {group_label}");

        let factors: Vec<Factor> = Factor::all()
            .into_iter()
            .filter(|factor| scores.iter().any(|score| score.values.contains_key(factor)))
            .collect();
        let torpedo_factors: Vec<TorpedoFactor> = TorpedoFactor::ALL
            .into_iter()
            .filter(|factor| {
                scores.iter().flat_map(|score| &score.torpedoes).any(|torpedo| torpedo.values.contains_key(factor))
            })
            .collect();
        let torpedo_pool = |factor: TorpedoFactor| {
            scores
                .iter()
                .flat_map(|score| &score.torpedoes)
                .filter_map(move |torpedo| torpedo.values.get(&factor).copied())
        };
        let listed: Vec<usize> = if named.is_empty() {
            (0..group.len()).collect()
        } else {
            named
                .iter()
                .filter_map(|ship| group.iter().position(|other| other.param.index() == ship.param.index()))
                .collect()
        };

        let ships = listed
            .into_iter()
            .map(|index| {
                let ship = group[index];
                let score = &scores[index];
                let mut outliers = Vec::new();

                let mut values = Vec::new();
                for factor in &factors {
                    let Some(&value) = score.values.get(factor) else {
                        continue;
                    };
                    let meta = factor.meta();
                    let standing = Standing::new(
                        value,
                        meta.better,
                        scores.iter().filter_map(|other| other.values.get(factor).copied()),
                    );
                    if let Some(outlier) = standing.outlier(&group_label) {
                        outliers.push(format!("{} {}: {outlier}", meta.key, meta.with_unit(value)));
                    }
                    values.push((meta.key, standing.ranked(&number(value), &group_label)));
                }

                let torpedoes = score
                    .torpedoes
                    .iter()
                    .map(|torpedo| {
                        let mut values = Vec::new();
                        for factor in &torpedo_factors {
                            let Some(&value) = torpedo.values.get(factor) else {
                                continue;
                            };
                            let meta = factor.meta();
                            let standing = Standing::new(value, meta.better, torpedo_pool(*factor));
                            if let Some(outlier) = standing.outlier(&torpedo_scope) {
                                outliers.push(format!(
                                    "torpedo {} {} {}: {outlier}",
                                    torpedo.name,
                                    meta.key,
                                    meta.with_unit(value)
                                ));
                            }
                            values.push((meta.key, standing.ranked(&number(value), &torpedo_scope)));
                        }

                        TorpedoOption {
                            torpedo: torpedo.name.clone(),
                            modules: torpedo.modules.clone(),
                            values: FactorValues(values),
                            damage_by_distance: torpedo.damage_by_distance.clone(),
                            builds: torpedo.builds.clone(),
                        }
                    })
                    .collect();

                let unique_upgrade = score.unique_upgrade.as_ref().map(|name| {
                    if score.unique_used.is_empty() {
                        format!("{name} (unique upgrade), not taken for any factor compared")
                    } else {
                        let used: Vec<&str> = score.unique_used.iter().map(|factor| factor.meta().key).collect();
                        format!("{name} (unique upgrade), taken for: {}", used.join(", "))
                    }
                });

                ComparedShip {
                    ship: ship.label(),
                    replicas: ship.replicas.clone(),
                    values: FactorValues(values),
                    outliers,
                    torpedoes,
                    unique_upgrade,
                    consumables: score.consumables.clone(),
                    special_abilities: score.special_abilities.clone(),
                }
            })
            .collect();

        Ok(ShipComparisonOutput {
            interpretations,
            group: group_label,
            ranked: group.len(),
            test: group.iter().filter(|ship| ship.status() == ShipStatus::Test).count(),
            torpedo_options: scores.iter().map(|score| score.torpedoes.len()).sum(),
            reference_range_km: reference_range,
            factors: factors.iter().map(|factor| factor.meta().legend()).collect(),
            torpedo_factors: torpedo_factors.iter().map(|factor| factor.meta().legend()).collect(),
            ships,
            notes: {
                let mut notes = NOTES.to_vec();
                if !anti_aircraft {
                    notes.push(
                        "Anti-aircraft is not compared: the server has no cache directory to read raw GameParams into.",
                    );
                }
                notes
            },
        })
    }

    /// Every factor of one ship but protection, on its best-fit builds.
    fn ship_scores(&self, ship: &Ship, reference_range: Km, anti_aircraft: bool) -> Result<ShipScores, QueryError> {
        let planner = Planner::new(self, ship);
        let stock = self.effective_modifiers(ship, &self.stock_loadout())?;
        let stock_bundle = stock.build.modifiers.bundle();
        let unique = self.upgrades.iter().find(|upgrade| upgrade.is_unique() && upgrade.fits(ship));
        let mut scores = ShipScores {
            values: HashMap::new(),
            unique_used: Vec::new(),
            unique_upgrade: unique.map(|upgrade| upgrade.name.clone()),
            consumables: self.consumable_summary(ship, stock_bundle),
            special_abilities: self.special_ability_summary(ship),
            torpedoes: Vec::new(),
        };
        let unique_label = unique.map(|upgrade| upgrade.build_label());
        let mut values: HashMap<Factor, f32> = HashMap::new();
        let mut unique_used = Vec::new();
        let mut record = |factor: Factor, value: f32, assumed: &[String]| {
            values.insert(factor, factor.rounded(value));
            if unique_label.as_ref().is_some_and(|label| assumed.contains(label)) {
                unique_used.push(factor);
            }
        };

        let plans: [(Factor, CardValue<'_>); 11] = [
            (Factor::Health, &|outcome| Some(outcome.stats.durability.as_ref()?.health?.value())),
            (Factor::MainBatteryReload, &|outcome| Some(outcome.stats.artillery.as_ref()?.reload_time?.value())),
            (Factor::MainBatteryRange, &|outcome| Some(outcome.stats.artillery.as_ref()?.range?.value())),
            (Factor::TurretTraverse, &|outcome| {
                Some(outcome.stats.artillery.as_ref()?.gun.as_ref()?.rotation_time?.value())
            }),
            (Factor::TorpedoReload, &|outcome| Some(outcome.stats.torpedoes.as_ref()?.reload_time?.value())),
            (Factor::Concealment, &|outcome| Some(outcome.stats.visibility.as_ref()?.sea_detection?.value())),
            (Factor::AirConcealment, &|outcome| Some(outcome.stats.visibility.as_ref()?.air_detection?.value())),
            (Factor::Speed, &|outcome| Some(outcome.stats.mobility.as_ref()?.speed?.value())),
            (Factor::RudderShift, &|outcome| Some(outcome.stats.mobility.as_ref()?.rudder_time?.value())),
            (Factor::TurningCircle, &|outcome| Some(outcome.stats.mobility.as_ref()?.turning_radius?.value())),
            (Factor::SecondaryRange, &|outcome| Some(outcome.stats.secondaries.as_ref()?.range.value())),
        ];
        for (factor, extract) in plans {
            let rounded = |outcome: &Outcome| extract(outcome).map(|value| Amount(round2(value)));
            let plan = StatPlan::new(factor.meta().key.to_owned(), factor.meta().better, &rounded);
            if let Some(builds) = planner.best(&plan)? {
                record(factor, builds.best.value.0, &builds.best.assumed);
            }
        }

        // Measured within the stock firing range; past it no build can shoot.
        let dispersion_range = match self.stock_ranges(ship)?.main_battery {
            Some(stock) => Km::new(reference_range.value().min(stock.value())),
            None => reference_range,
        };
        let dispersion = |outcome: &Outcome| {
            extended_horizontal_dispersion(&planner, outcome, dispersion_range).map(|meters| Amount(meters.value()))
        };
        if let Some(builds) = planner.best(&StatPlan::new("dispersion".to_owned(), Better::Lower, &dispersion))? {
            record(Factor::Dispersion, builds.best.value.0, &builds.best.assumed);
        }
        let sigma = |outcome: &Outcome| {
            artillery_sigma(&ship.param, planner.selection(outcome.selection), &outcome.bundle)
                .map(|sigma| Amount(round2(sigma)))
        };
        if let Some(builds) = planner.best(&StatPlan::new("sigma".to_owned(), Better::Higher, &sigma))? {
            record(Factor::Sigma, builds.best.value.0, &builds.best.assumed);
        }
        for kind in AmmoKind::ALL {
            let alpha = |outcome: &Outcome| main_battery_salvo(&planner, outcome, kind).map(Amount);
            let plan = StatPlan::new(kind.key("alpha").to_owned(), Better::Higher, &alpha);
            if let Some(builds) = planner.best(&plan)? {
                record(Factor::Alpha(kind), builds.best.value.0, &builds.best.assumed);
            }
        }

        let rates = self.rates(&planner)?;
        for ammo in &rates.main_battery_damage {
            let Some(kind) = AmmoKind::from_label(&ammo.ammo) else {
                continue;
            };
            let best = &ammo.rates.builds.best;
            record(Factor::DamagePerMinute(kind), best.value.damage_per_minute.value(), &best.assumed);
        }
        if !rates.secondary_damage.is_empty() {
            let total: f32 =
                rates.secondary_damage.iter().map(|ammo| ammo.rates.builds.best.value.damage_per_minute.value()).sum();
            let assumed: Vec<String> =
                rates.secondary_damage.iter().flat_map(|ammo| ammo.rates.builds.best.assumed.iter().cloned()).collect();
            record(Factor::SecondaryDamagePerMinute, total, &assumed);
        }
        if let Some(best) = rates.torpedo_damage.iter().map(|ammo| &ammo.rates.builds.best).max_by(|left, right| {
            left.value.damage_per_minute.value().total_cmp(&right.value.damage_per_minute.value())
        }) {
            record(Factor::TorpedoDamagePerMinute, best.value.damage_per_minute.value(), &best.assumed);
        }
        if let Some(torpedoes) = &rates.torpedoes_per_minute {
            let best = &torpedoes.rate.builds.best;
            record(Factor::TorpedoesPerMinute, best.value.torpedoes_per_minute.value(), &best.assumed);
        }
        if let Some(fires) = self.fire_rates(&planner)? {
            let best = &fires.total.builds.best;
            record(Factor::FiresPerMinute, best.value.fires_per_minute.value(), &best.assumed);
        }

        if let Some(range) = values.get(&Factor::MainBatteryRange).copied()
            && range >= reference_range.value()
        {
            let shells: Vec<Shell> =
                self.shells(ship).into_iter().filter(|shell| shell.battery == Battery::Main).collect();
            for kind in AmmoKind::ALL {
                let Some(shell) = largest_shell(&shells, kind.ammo()) else {
                    continue;
                };
                if let Some(time) = self.flight_time(shell, reference_range)? {
                    values.insert(Factor::FlightTime(kind), Factor::FlightTime(kind).rounded(time));
                }
            }
        }

        if let Some(arcs) = self.mount_arcs(ship, &planner, ComponentType::Artillery)? {
            values.insert(Factor::BowBarrels, round2(arcs.share(arcs.bow)));
            values.insert(Factor::SternBarrels, round2(arcs.share(arcs.stern)));
            if let Some(angle) = arcs.all_from_bow {
                values.insert(Factor::AllBearFromBow, f32::from(angle));
            }
            if let Some(angle) = arcs.all_from_stern {
                values.insert(Factor::AllBearFromStern, f32::from(angle));
            }
        }
        // Submarine torpedoes leave fixed tubes, not aimed launchers.
        if ship.class != ShipClass::Submarine
            && let Some(arcs) = self.mount_arcs(ship, &planner, ComponentType::Torpedoes)?
        {
            if let Some(angle) = arcs.any_from_bow {
                values.insert(Factor::TorpedoLaunchFromBow, f32::from(angle));
            }
            values.insert(Factor::BroadsideTubes, arcs.broadside as f32);
        }

        if anti_aircraft && let Some((damage, range)) = self.anti_aircraft(ship, &planner)? {
            values.insert(Factor::AntiAircraftDamage, damage.round());
            values.insert(Factor::AntiAircraftRange, round2(range.value()));
        }
        if let Some(heal) = self.repair_party_heal(ship, stock_bundle) {
            values.insert(Factor::RepairPartyHeal, heal);
        }

        scores.torpedoes = self.torpedo_scores(&planner, &rates)?;
        scores.values = values;
        scores.unique_used = unique_used;

        Ok(scores)
    }

    /// Each torpedo of every torpedo module, on its best-fit builds.
    fn torpedo_scores(
        &self,
        planner: &Planner<'_>,
        rates: &crate::rates::Rates,
    ) -> Result<Vec<TorpedoScores>, QueryError> {
        /// A torpedo and the first module combination that carries it.
        struct Carried {
            param: String,
            modules: Vec<String>,
            selection: usize,
        }

        // Selections come stock first, so a torpedo the stock modules carry
        // is listed without modules.
        let mut options: Vec<Carried> = Vec::new();
        for (selection, _) in planner.selections().enumerate() {
            let outcome = planner.bare(selection)?;
            for torpedo in outcome.stats.torpedoes.iter().flat_map(|torpedoes| &torpedoes.torpedoes) {
                if !options.iter().any(|option| option.param == torpedo.name) {
                    options.push(Carried {
                        param: torpedo.name.clone(),
                        modules: planner.selection_modules(selection).to_vec(),
                        selection,
                    });
                }
            }
        }

        let mut scores = Vec::new();
        for Carried { param, modules, selection } in options {
            let card = |outcome: &Outcome| TorpedoCard::of(outcome, &param);
            let mut values = HashMap::new();
            let mut build_lines = Vec::new();
            let plans: [(TorpedoFactor, CardValue<'_>); 5] = [
                (TorpedoFactor::Speed, &|outcome| card(outcome)?.speed),
                (TorpedoFactor::Range, &|outcome| card(outcome)?.range),
                (TorpedoFactor::Damage, &|outcome| card(outcome)?.damage),
                (TorpedoFactor::Detectability, &|outcome| card(outcome)?.visibility),
                (TorpedoFactor::ReactionTime, &|outcome| {
                    let torpedo = card(outcome)?;
                    Some(torpedo.visibility? * KM_TO_M / (torpedo.speed? * TORPEDO_METERS_PER_SECOND_PER_KNOT))
                }),
            ];
            for (factor, extract) in plans {
                let rounded = |outcome: &Outcome| extract(outcome).map(|value| Amount(round2(value)));
                let plan =
                    StatPlan::new(factor.meta().key.to_owned(), factor.meta().better, &rounded).on_selection(selection);
                if let Some(builds) = planner.best(&plan)? {
                    let best = factor.rounded(builds.best.value.0);
                    let base = factor.rounded(builds.base.0);
                    // The base already carries the torpedo's modules.
                    let assumed: Vec<&str> = builds
                        .best
                        .assumed
                        .iter()
                        .map(String::as_str)
                        .filter(|item| !modules.iter().any(|module| module == item))
                        .collect();
                    if !assumed.is_empty() && best != base {
                        let meta = factor.meta();
                        build_lines.push(format!(
                            "{} {} with {}; {} without",
                            meta.key,
                            meta.with_unit(best),
                            assumed.join(", "),
                            meta.with_unit(base)
                        ));
                    }
                    values.insert(factor, best);
                }
            }

            let name = self.projectile_name(&param);
            if let Some(ammo) = rates.torpedo_damage.iter().find(|ammo| ammo.ammo == name) {
                values.insert(
                    TorpedoFactor::DamagePerMinute,
                    ammo.rates.builds.best.value.damage_per_minute.value().round(),
                );
            }

            let projectile = self.projectile(&param)?;
            if let Some(flood) = projectile.uw_critical() {
                values.insert(TorpedoFactor::FloodCoefficient, round2(flood));
            }

            // The falloff points need the arming run, which depends on the
            // torpedo's speed on the first module combination carrying it.
            let speed = card(&*planner.bare(selection)?).and_then(|torpedo| torpedo.speed);
            let points = speed.and_then(|speed| falloff_points(&projectile, speed));
            if let Some(damage) = values.get(&TorpedoFactor::Damage).copied() {
                let last = points.as_ref().and_then(|points| points.last()).map_or(1.0, |point| point.coefficient);
                values.insert(TorpedoFactor::DamageAtMaxRange, (damage * last).round());
            }

            scores.push(TorpedoScores {
                name,
                modules,
                values,
                damage_by_distance: points.map(|points| describe_falloff(&points)),
                builds: build_lines,
            });
        }

        Ok(scores)
    }

    /// How a battery bears ahead and astern on the module combination with
    /// the most barrels. `None` without the battery or the hull's
    /// hardpoint facings.
    fn mount_arcs(
        &self,
        ship: &Ship,
        planner: &Planner<'_>,
        component: ComponentType,
    ) -> Result<Option<MountArcs>, QueryError> {
        let mut best: Option<MountArcs> = None;
        'selections: for selection in planner.selections() {
            let facings = match self.hardpoint_facings(ship, selection) {
                Ok(facings) => facings,
                Err(QueryError::HullFacingsUnavailable { .. }) => return Ok(None),
                Err(error) => return Err(error),
            };
            let mut mounts = Vec::new();
            for (mount, barrels) in aiming_mounts(&ship.param, selection, component) {
                // A hull model without the facing of every mount says
                // nothing about how the battery bears.
                let Some(&facing) = facings.get(mount.hp_name()) else {
                    continue 'selections;
                };
                mounts.push(FacedMount { mount, barrels, facing });
            }
            if let Some(arcs) = MountArcs::of(&mounts)
                && best.is_none_or(|best| arcs.barrels > best.barrels)
            {
                best = Some(arcs);
            }
        }

        Ok(best)
    }

    /// Flight time of `shell` to `range`, on the in-game clock.
    fn flight_time(&self, shell: &Shell, range: Km) -> Result<Option<f32>, QueryError> {
        let projectile = self.projectile(&shell.param)?;
        let Some(params) = ShellParams::from_shell_info(&projectile.to_shell_info(shell.param.clone())) else {
            return Ok(None);
        };

        Ok(solve_for_range(&params, range.to_meters()).map(|impact| impact.time_to_target.value()))
    }

    /// Continuous anti-aircraft damage per second and the reach of the
    /// longest aura of the anti-aircraft and secondary components, stock,
    /// on the hull with the most damage.
    fn anti_aircraft(&self, ship: &Ship, planner: &Planner<'_>) -> Result<Option<(f32, Km)>, QueryError> {
        let Some(vehicle) = ship.param.vehicle() else {
            return Ok(None);
        };
        let hulls: Vec<&str> = planner.selections().filter_map(|selection| selection.hull.as_deref()).collect();
        if hulls.is_empty() {
            return Ok(None);
        }
        let Some(raw) = self.raw_entry(ship.param.name())? else {
            return Ok(None);
        };

        let mut best: Option<(f32, Km)> = None;
        for hull in hulls {
            let Some(config) = vehicle.hull_upgrade(hull) else {
                continue;
            };
            let mut damage = 0.0;
            let mut reach: f32 = 0.0;
            for component in [ComponentType::AirDefense, ComponentType::Atba] {
                let Some(fields) = config.component_names.get(&component).and_then(|name| raw.get(name)?.as_object())
                else {
                    continue;
                };
                for aura in fields.values() {
                    let number =
                        |key: &str| aura.get(key).and_then(serde_json::Value::as_f64).map(|value| value as f32);
                    let (Some(area), Some(period)) = (number("areaDamage"), number("areaDamagePeriod")) else {
                        continue;
                    };
                    if area <= 0.0 || period <= 0.0 {
                        continue;
                    }
                    damage += area / period;
                    reach = reach.max(number("maxDistance").unwrap_or(0.0) / KM_TO_M);
                }
            }
            if damage > 0.0 && best.is_none_or(|(best, _)| damage > best) {
                best = Some((damage, Km::new(reach)));
            }
        }

        Ok(best)
    }

    /// Percent of maximum HP one stock Repair Party charge restores.
    fn repair_party_heal(&self, ship: &Ship, bundle: &ModifierBundle) -> Option<f32> {
        let slots = ship.param.vehicle()?.abilities()?;

        slots.iter().flatten().find_map(|(ability, variant)| {
            let param = self.provider().game_param_by_name(ability)?;
            let category = param.ability()?.get_category(variant)?;
            if category.consumable_type_raw() != "regenCrew" {
                return None;
            }

            let stats = consumable_stats(self.consumable_name(ability, category), category, bundle);
            Some(round2(stats.heal_percent_per_s?.value() * stats.action_time_s?.value()))
        })
    }

    /// "Repair Party (4 charges, 80 s reload)" per stock consumable slot.
    fn consumable_summary(&self, ship: &Ship, bundle: &ModifierBundle) -> Vec<String> {
        let slots = ship.param.vehicle().and_then(|vehicle| vehicle.abilities()).unwrap_or_default();

        slots
            .iter()
            .filter_map(|slot| {
                let options: Vec<String> = slot
                    .iter()
                    .filter_map(|(ability, variant)| {
                        let param = self.provider().game_param_by_name(ability)?;
                        let category = param.ability()?.get_category(variant)?;
                        let stats = consumable_stats(self.consumable_name(ability, category), category, bundle);
                        let charges = match stats.charges {
                            Charges::Limited(1) => "1 charge".to_owned(),
                            Charges::Limited(count) => format!("{count} charges"),
                            Charges::Unlimited => "unlimited charges".to_owned(),
                        };
                        Some(format!("{} ({charges}, {} s reload)", stats.name, stats.reload_s.value()))
                    })
                    .collect();
                (!options.is_empty()).then(|| options.join(" or "))
            })
            .collect()
    }

    /// "Combat Instructions: <title> (<effects>)" or "<fire mode> (<effects>)".
    fn special_ability_summary(&self, ship: &Ship) -> Vec<String> {
        self.special_abilities(ship)
            .into_iter()
            .map(|ability| match ability {
                SpecialAbility::CombatInstructions(info) => {
                    format!("Combat Instructions: {} ({})", info.name, info.effects.join("; "))
                }
                SpecialAbility::AlternativeFireMode(info) if info.effects.is_empty() => info.name,
                SpecialAbility::AlternativeFireMode(info) => format!("{} ({})", info.name, info.effects.join("; ")),
            })
            .collect()
    }
}

/// Damage of one salvo of every main battery barrel of `kind` on `outcome`'s
/// modules, rounded to whole HP.
fn main_battery_salvo(planner: &Planner<'_>, outcome: &Outcome, kind: AmmoKind) -> Option<f32> {
    let artillery = outcome.stats.artillery.as_ref()?;
    let shells: Vec<&ShellStats> =
        artillery.shells.iter().filter(|shell| AmmoKind::of_shell(shell) == Some(kind)).collect();
    if shells.is_empty() {
        return None;
    }

    let selection: &ShipUpgradeSelection = planner.selection(outcome.selection);
    let mounts = aiming_mounts(&planner.ship().param, selection, ComponentType::Artillery);
    let salvo: f32 = mounts.iter().map(|(mount, barrels)| mount_salvo(mount.ammo(), *barrels, &shells).value()).sum();

    (salvo > 0.0).then(|| salvo.round())
}

/// A share of full torpedo damage from a distance on.
struct FalloffPoint {
    distance: Km,
    coefficient: f32,
}

/// The `distanceOfDamage` points of a torpedo as distances run from launch,
/// nearest first: each pair's distance after the arming run, as the client
/// places `distanceOfMaxDamage` (FactoryTorpedoes.py:131-147). `None` for a
/// torpedo whose damage does not depend on distance.
fn falloff_points(projectile: &wowsunpack::game_params::types::Projectile, speed: f32) -> Option<Vec<FalloffPoint>> {
    let pairs = projectile.distance_of_damage().filter(|pairs| !pairs.is_empty())?;
    let run = wowsunpack::game_params::ttx::factories::torpedo_arming_run(
        projectile,
        wowsunpack::game_params::ttx::model::Knots::from(speed),
    )?;

    let mut points: Vec<FalloffPoint> = pairs
        .iter()
        .map(|&(distance, coefficient)| FalloffPoint {
            distance: Km::new(round2((run + distance) * BW_TO_BALLISTIC / KM_TO_M)),
            coefficient,
        })
        .collect();
    points.sort_by(|left, right| left.distance.value().total_cmp(&right.distance.value()));

    Some(points)
}

/// "100% of full damage up to 8 km, 50% from 8.1 km".
fn describe_falloff(points: &[FalloffPoint]) -> String {
    let parts: Vec<String> = points
        .iter()
        .enumerate()
        .map(|(index, point)| {
            let share = number(point.coefficient * 100.0);
            let distance = number(point.distance.value());
            if index == 0 {
                format!("{share}% of full damage up to {distance} km")
            } else {
                format!("{share}% from {distance} km")
            }
        })
        .collect();

    parts.join(", ")
}

/// A factor value for the build planner.
#[derive(Debug, Clone, Copy)]
struct Amount(f32);

impl StatAmount for Amount {
    fn amount(self) -> f32 {
        self.0
    }
}

/// "tier 10" or "tier 9-11", as armor_assessment names its peers.
fn tier_label(tiers: TierRange) -> String {
    if tiers.min == tiers.max {
        format!("tier {}", tiers.min.0)
    } else {
        format!("tier {}-{}", tiers.min.0, tiers.max.0)
    }
}

/// Up to two decimals, without trailing zeros.
fn number(value: f32) -> String {
    let text = format!("{:.2}", (f64::from(value) * 100.0).round() / 100.0);
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

/// Two decimals. Rounds in f64, as hundredths of a large f32 value such as a
/// damage rate do not fit an f32 exactly.
fn round2(value: f32) -> f32 {
    ((f64::from(value) * 100.0).round() / 100.0) as f32
}

fn is_zero(count: &usize) -> bool {
    *count == 0
}
