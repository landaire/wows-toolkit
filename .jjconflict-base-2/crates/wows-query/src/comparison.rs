//! Ships of one class and tier range compared on the factors that decide
//! their strength, each ranked within the group, with labels for clear
//! outliers: per armament, per torpedo option and, for cruisers and
//! battleships, against the shooters of their matchmaking. The one composite
//! score is the battle_impact judgment, whose weights are stated with it.

use std::collections::BTreeMap;
use std::collections::HashMap;

use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use serde::ser::SerializeMap;
use wows_core::units::Degrees;
use wows_core::units::Km;
use wows_core::units::Meters;
use wows_core::units::Millimeters;
use wows_core::units::Percent;
use wows_ship_stats::AaRating;
use wows_ship_stats::Bearing;
use wows_ship_stats::bearing::mount_bears;
use wows_ship_stats::mount_salvo;
use wows_ship_stats::mounts::aiming_mounts;
use wowsunpack::ballistics::ShellParams;
use wowsunpack::ballistics::is_overmatch;
use wowsunpack::ballistics::solve_for_range;
use wowsunpack::game_params::keys::ComponentType;
use wowsunpack::game_params::ttx::anti_air::AuraBand;
use wowsunpack::game_params::ttx::anti_air::anti_air;
use wowsunpack::game_params::ttx::constants::BW_KNOTS_TO_MPS;
use wowsunpack::game_params::ttx::constants::BW_TO_BALLISTIC;
use wowsunpack::game_params::ttx::constants::KM_TO_M;
use wowsunpack::game_params::ttx::model::Hp;
use wowsunpack::game_params::ttx::model::ShellStats;
use wowsunpack::game_params::ttx::model::TorpedoStats;
use wowsunpack::game_params::ttx::modifiers::ModifierBundle;
use wowsunpack::game_params::ttx::orchestration::artillery_sigma;
use wowsunpack::game_params::ttx::selection::ShipUpgradeSelection;
use wowsunpack::game_params::types::GameParamProvider;
use wowsunpack::game_params::types::MountPoint;

use crate::armor::zone_thickness;
use crate::armor_assessment::Aspect;
use crate::armor_assessment::Matchup;
use crate::armor_assessment::Measured;
use crate::armor_assessment::RangeBand;
use crate::armor_assessment::ShipMatchups;
use crate::armor_assessment::ShooterRole;
use crate::armor_assessment::class_plural;
use crate::armor_assessment::parallel_map;
use crate::armor_trace::largest_shell;
use crate::battle_impact::ImpactSummary;
use crate::battle_impact::summaries;
use crate::builds::Better;
use crate::builds::Charges;
use crate::builds::Outcome;
use crate::builds::Planner;
use crate::builds::StatAmount;
use crate::builds::StatPlan;
use crate::consumables::Capacity;
use crate::consumables::consumable_stats;
use crate::consumables::repair_heal;
use crate::data::GameData;
use crate::error::QueryError;
use crate::names::Interpretations;
use crate::shells::Ammo;
use crate::shells::Battery;
use crate::shells::Shell;
use crate::ship_variants::ShipCategory;
use crate::ship_variants::listing_order;
use crate::ships::Ship;
use crate::ships::ShipClass;
use crate::ships::ShipFilter;
use crate::ships::ShipStatus;
use crate::ships::Tier;
use crate::ships::TierRange;
use crate::special_abilities::SpecialAbility;
use crate::stats::extended_horizontal_dispersion;
use crate::upgrades::Upgrade;

/// Ships besides the one labelled a group must hold before a value is
/// labelled an outlier: in smaller groups the extremes say little.
const OUTLIER_MIN_OTHERS: usize = 4;

/// Share of the other ships a value must beat, or lose to, to be labelled
/// an outlier.
const OUTLIER_SHARE: f32 = 0.9;

/// The note on torpedo reaction time, left out of the tool output when no
/// named ship carries torpedoes.
const REACTION_TIME_NOTE: &str = "Torpedo reaction_time = detectability / speed, with speed in knots x 2.69 m/s per knot (the client's BW_KNOTS_TO_MPS times 30 m per BigWorld unit).";

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

    /// Ships to give full entries, e.g. ["Incomparable", "C. Colombo"]; name the ship asked about. Without ships only a summary of the group is returned. Ranks still compare each with the whole group, which also takes in every ship named. Without tiers, the group spans the tiers of the ships named. Matchups against shooter groups are listed only for ships named.
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

    /// Names of the ranked ships, in listing order.
    /// [`GameData::ship_comparison_summary`] gives them only without ships
    /// named.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub ship_names: Vec<String>,

    /// Per factor, the best and the worst ships of the group with their
    /// values, best first and worst first: three each without ships named,
    /// one each with them.
    pub leaders: FactorLeaderTable,

    /// The same among every torpedo option of the group.
    #[serde(skip_serializing_if = "FactorLeaderTable::is_empty")]
    pub torpedo_leaders: FactorLeaderTable,

    /// Full entries: the ships named, or with none named every ranked ship.
    /// [`GameData::ship_comparison_summary`], which the tool returns, lists
    /// none without ships named.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub ships: Vec<ComparedShip>,

    /// Every pair of named ships of one class, factor by factor.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub head_to_head: Vec<HeadToHead>,

    pub notes: Vec<&'static str>,
}

/// Two named ships of one class compared factor by factor, every factor
/// counting the same.
#[derive(Debug, Clone, Serialize)]
pub struct HeadToHead {
    /// The two ships, `first` and `second` below.
    pub ships: [String; 2],

    /// Factors both ships have.
    pub compared: usize,

    /// Factor keys each ship is better on, and those they tie on.
    pub first_better: Vec<&'static str>,
    pub second_better: Vec<&'static str>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tied: Vec<&'static str>,

    /// The largest leads of each ship, the gap as a share of the larger
    /// value: "dpm_ap 585744 vs 453147 HP/min (23% apart)".
    pub first_largest_leads: Vec<String>,
    pub second_largest_leads: Vec<String>,

    /// The split in one sentence: "On paper A is better: ..." when one ship
    /// leads on at least twice as many factors as the other, else "Mixed:
    /// ...".
    pub verdict: String,
}

/// The best and worst values of one factor in a group.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FactorLeaders {
    /// "Moskva (Russia) 14.14 km", best first.
    pub best: Vec<String>,

    /// Worst first.
    pub worst: Vec<String>,
}

/// Leaders per factor in factor order, serialized as a map from factor key.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FactorLeaderTable(pub Vec<(&'static str, FactorLeaders)>);

impl FactorLeaderTable {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl Serialize for FactorLeaderTable {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (key, leaders) in &self.0 {
            map.serialize_entry(key, leaders)?;
        }
        map.end()
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ComparedShip {
    /// "T10 C. Colombo (battleship, Italy, premium)", ending ", premium" or
    /// ", test" for a ship not of the tech tree.
    pub ship: String,

    /// tech_tree, premium (premium and special ships, outside the tech tree)
    /// or test.
    pub category: ShipCategory,

    /// A cruiser's type by its largest main battery caliber; absent for
    /// other classes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cruiser_type: Option<CruiserType>,

    /// GameParams index, e.g. "PISB110". Not in the tool output.
    #[serde(skip)]
    pub index: String,

    /// Not in the tool output.
    #[serde(skip)]
    pub tier: Tier,

    /// Premium replicas of this ship, left out of the ranks.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub replicas: Vec<String>,

    /// Per factor, "value (rank r/n, pNN)".
    pub values: FactorValues,

    /// `values` as numbers, in the same order. Not in the tool output.
    #[serde(skip)]
    pub standings: Vec<FactorStanding>,

    /// Values that beat, or lose to, at least 90% of the other ships, or
    /// torpedo options or peers in a matchup, in groups of five or more.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub outliers: Vec<String>,

    /// Each torpedo the ship's torpedo modules carry.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub torpedoes: Vec<TorpedoOption>,

    /// Rates against each shooter group of the ship's tier at the band the
    /// group fights at, per aspect; ships named only.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub matchups: Vec<MatchupLine>,

    /// Play style the factors point to, with the numbers behind it.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub signals: Vec<String>,

    /// The battle_impact score and its components; absent for carriers and
    /// submarines.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub battle_impact: Option<ImpactSummary>,

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

    /// `values` as numbers, in the same order. Not in the tool output.
    #[serde(skip)]
    pub standings: Vec<FactorStanding>,

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

#[derive(Debug, Clone, Serialize)]
pub struct MatchupLine {
    /// e.g. "battleship AP 406 mm (Iowa), 12-14 km".
    pub shooter: String,

    /// Per aspect, the citadel, damaging and overpenetration rates with
    /// their ranks among the peers.
    pub aspects: FactorValues,
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

/// One factor of a ship or torpedo option and where its value ranks.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FactorStanding {
    /// Factor key, as `factors` and `torpedo_factors` name it, e.g. "dispersion".
    pub factor: &'static str,

    /// In `unit`, unrounded.
    pub value: f32,
    pub unit: &'static str,
    pub better: FactorDirection,
    pub rank: GroupRank,
    pub scope: RankScope,

    /// Rank among the cruisers of the same type in the group (the same tier
    /// too for per-tier factors). `None` outside cruiser groups, for torpedo
    /// options, and when every cruiser of the group is of one type.
    pub type_rank: Option<GroupRank>,
}

/// Which values of a factor are better.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FactorDirection {
    Lower,
    Higher,
}

/// The ships a factor ranks among.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RankScope {
    /// Every ship of the group, or every torpedo option of the group.
    Group,

    /// The ships of the group of the ship's own tier.
    Tier,
}

/// Which ships of a group get full entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Listing {
    /// The ships named, or every ship when none is named.
    Named,

    /// Every ship of the group.
    Every,
}

/// Where a value ranks among the values of its group, its own included: the
/// ships of the group, of the ship's tier for per-tier factors, or every
/// torpedo option of the group.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct GroupRank {
    /// From 1, the best first; equal values share a position.
    pub position: usize,

    /// Values ranked, the n of "rank r/n".
    pub count: usize,

    /// Other values better than, worse than and equal to this one.
    pub better: usize,
    pub worse: usize,
    pub tied: usize,

    /// Set when the value beats, or loses to, at least 90% of the others,
    /// with at least 4 others.
    pub outlier: Option<Outlier>,
}

impl GroupRank {
    /// The rank of `value` among `values`, its own included.
    fn new(value: f32, better: Better, values: impl Iterator<Item = f32>) -> Self {
        let mut count: usize = 0;
        let mut above: usize = 0;
        let mut below: usize = 0;
        for other in values {
            count += 1;
            if better.improves(other, value) {
                above += 1;
            } else if better.improves(value, other) {
                below += 1;
            }
        }
        let others = count.saturating_sub(1);

        let outlier = if others < OUTLIER_MIN_OTHERS {
            None
        } else if above as f32 >= OUTLIER_SHARE * others as f32 {
            Some(Outlier::Worse)
        } else if below as f32 >= OUTLIER_SHARE * others as f32 {
            Some(Outlier::Better)
        } else {
            None
        };

        Self {
            position: 1 + above,
            count,
            better: above,
            worse: below,
            tied: others.saturating_sub(above + below),
            outlier,
        }
    }

    /// "value (rank r/n, pNN)": pNN is the share of the other values the
    /// ship's beats.
    fn ranked(&self, shown: &str, scope: &str) -> String {
        format!("{shown} ({})", self.position_text(scope))
    }

    /// "rank r/n, pNN", or "the only one of the {scope} with it".
    fn position_text(&self, scope: &str) -> String {
        match self.beaten() {
            None => format!("the only one of the {scope} with it"),
            Some(beaten) => format!("rank {}/{}, p{}", self.position, self.count, beaten.value().round()),
        }
    }

    /// [`GroupRank::position_text`] followed by `within`, the rank among
    /// cruisers of `cruiser` type: "rank 3/56, p96; heavy cruisers rank 9/24, p65".
    fn position_text_with_type(&self, scope: &str, cruiser: CruiserType, within: &GroupRank) -> String {
        let typed = match within.beaten() {
            None => format!("the only {} with it", cruiser.label()),
            Some(_) => format!("{} {}", cruiser.plural(), within.position_text(cruiser.plural())),
        };

        format!("{}; {typed}", self.position_text(scope))
    }

    /// For an outlier, "strength" or "weakness" and where the value stands:
    /// "the lowest of the 43 tier 10 cruisers (rank 1/43)" or "higher than
    /// 96% of the 81 other tier 9-11 battleships (rank 79/82)". `better` is
    /// the direction the value was ranked in.
    fn outlier_line(&self, scope: &str, better: Better) -> Option<(&'static str, String)> {
        let outlier = self.outlier?;
        let others = self.others();
        let (verdict, beyond) = match outlier {
            Outlier::Better => ("strength", self.worse),
            Outlier::Worse => ("weakness", self.better),
        };
        let low = matches!((outlier, better), (Outlier::Better, Better::Lower) | (Outlier::Worse, Better::Higher));
        let (extreme, comparative) = if low { ("lowest", "lower") } else { ("highest", "higher") };
        let standing = if beyond == others {
            format!("the {extreme} of the {} {scope}", self.count)
        } else if beyond + self.tied == others {
            format!("tied for the {extreme} of the {} {scope}", self.count)
        } else {
            format!(
                "{comparative} than {}% of the {others} other {scope}",
                (beyond as f32 * 100.0 / others as f32).round()
            )
        };

        Some((verdict, format!("{standing} (rank {}/{})", self.position, self.count)))
    }

    /// Values ranked besides this one.
    pub fn others(&self) -> usize {
        self.count.saturating_sub(1)
    }

    /// Share of the other values this one beats, the pNN of "rank r/n,
    /// pNN" unrounded. `None` when it is the only value.
    pub fn beaten(&self) -> Option<Percent> {
        self.share(self.worse as f32)
    }

    /// Share of the other values this one beats, ties counting half.
    /// `None` when it is the only value.
    pub fn beaten_ties_half(&self) -> Option<Percent> {
        self.share(self.worse as f32 + self.tied as f32 / 2.0)
    }

    /// Share of the other values better than this one. `None` when it is
    /// the only value.
    pub fn share_better(&self) -> Option<Percent> {
        self.share(self.better as f32)
    }

    fn share(&self, beaten: f32) -> Option<Percent> {
        let others = self.others();

        (others > 0).then(|| Percent::new(beaten * 100.0 / others as f32))
    }
}

/// Which way a value stands out from its group.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outlier {
    Better,
    Worse,
}

/// A cruiser's type by its largest main battery caliber. The game draws no
/// such line; the thresholds fall in the gaps of the calibers cruisers carry:
/// up to 165 mm (the 152 to 155 mm guns of most light cruisers, Jurien's
/// 165 mm), 180 to 254 mm (Soviet 180 mm, the common 203 mm, Moskva's
/// 220 mm, Henri IV's 240 mm, Napoli's 254 mm), and 280 mm and up (Graf
/// Spee's 283 mm, Alaska's and Stalingrad's 305 mm, Marseille's 330 mm,
/// Siegfried's and Sevastopol's 380 mm).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CruiserType {
    Light,
    Heavy,
    Large,
}

impl CruiserType {
    pub fn from_caliber(caliber: Millimeters) -> Self {
        // Below 170 mm, and up to 260 mm: each bound sits in a gap no
        // cruiser's guns fall in.
        const LIGHT_BELOW: f32 = 170.0;
        const HEAVY_UP_TO: f32 = 260.0;

        if caliber.value() < LIGHT_BELOW {
            Self::Light
        } else if caliber.value() <= HEAVY_UP_TO {
            Self::Heavy
        } else {
            Self::Large
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Light => "light cruiser",
            Self::Heavy => "heavy cruiser",
            Self::Large => "large cruiser",
        }
    }

    fn plural(self) -> &'static str {
        match self {
            Self::Light => "light cruisers",
            Self::Heavy => "heavy cruisers",
            Self::Large => "large cruisers",
        }
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

    fn standing(&self, value: f32, rank: GroupRank, scope: RankScope, type_rank: Option<GroupRank>) -> FactorStanding {
        let better = match self.better {
            Better::Lower => FactorDirection::Lower,
            Better::Higher => FactorDirection::Higher,
        };

        FactorStanding { factor: self.key, value, unit: self.unit, better, rank, scope, type_rank }
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
    RepairPartyTotalHeal,
    DamageControlReload,
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
    AntiAircraftRating,
    AntiAircraftLongDps,
    AntiAircraftFlak,
    Concealment,
    AirConcealment,
    RadarRange,
    RadarDuration,
    HydroRange,
    TorpedoDetection,
    SmokeLifetime,
    Speed,
    RudderShift,
    TurningCircle,
    BattleshipApCitadel,
    BattleshipApOverpenetration,
    CruiserHeDamaging,
    BowOvermatch,
    SternOvermatch,
}

impl Factor {
    fn all() -> Vec<Self> {
        let mut factors = vec![
            Self::Health,
            Self::RepairPartyHeal,
            Self::RepairPartyTotalHeal,
            Self::DamageControlReload,
            Self::MainBatteryReload,
            Self::MainBatteryRange,
        ];
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
            Self::AntiAircraftRating,
            Self::AntiAircraftLongDps,
            Self::AntiAircraftFlak,
            Self::Concealment,
            Self::AirConcealment,
            Self::RadarRange,
            Self::RadarDuration,
            Self::HydroRange,
            Self::TorpedoDetection,
            Self::SmokeLifetime,
            Self::Speed,
            Self::RudderShift,
            Self::TurningCircle,
            Self::BattleshipApCitadel,
            Self::BattleshipApOverpenetration,
            Self::CruiserHeDamaging,
            Self::BowOvermatch,
            Self::SternOvermatch,
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
            Self::RepairPartyTotalHeal => {
                ("repair_party_total_heal", "HP", Better::Higher, "HP every Repair Party charge restores together")
            }
            Self::DamageControlReload => ("damage_control_reload", "s", Better::Lower, "Damage Control Party reload"),
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
            Self::AntiAircraftRating => (
                "aa_rating",
                "hp",
                Better::Higher,
                "AA rating: damage the AA deals to one plane flying straight in at 150 knots from the farthest aura to the ship, continuous damage times hit chance plus a tenth of the flak bursts spawned; a judgment, see ship_stats anti_aircraft",
            ),
            Self::AntiAircraftLongDps => (
                "aa_long_dps",
                "HP/s",
                Better::Higher,
                "continuous damage per second of the long-range AA auras, before hit chance",
            ),
            Self::AntiAircraftFlak => (
                "aa_flak",
                "hp",
                Better::Higher,
                "damage of one flak salvo if every burst hits: bursts times burst damage",
            ),
            Self::Concealment => ("concealment", "km", Better::Lower, "detectability by ships"),
            Self::AirConcealment => ("air_concealment", "km", Better::Lower, "detectability by aircraft"),
            Self::RadarRange => ("radar_range", "km", Better::Higher, "Surveillance Radar range"),
            Self::RadarDuration => ("radar_duration", "s", Better::Higher, "Surveillance Radar action time"),
            Self::HydroRange => ("hydro_range", "km", Better::Higher, "Hydroacoustic Search range against ships"),
            Self::TorpedoDetection => ("torpedo_detection", "km", Better::Higher, "Hydroacoustic Search torpedo range"),
            Self::SmokeLifetime => ("smoke_lifetime", "s", Better::Higher, "time a smoke cloud lasts"),
            Self::Speed => ("speed", "kn", Better::Higher, "top speed"),
            Self::RudderShift => ("rudder_shift", "s", Better::Lower, "rudder shift time"),
            Self::TurningCircle => ("turning_circle", "m", Better::Lower, "turning circle radius"),
            Self::BattleshipApCitadel => (
                "bb_ap_citadel",
                "%",
                Better::Lower,
                "broadside AP hits of the most common battleship caliber of its matchmaking at 12 to 14 km that detonate in the citadel, from armor_assessment",
            ),
            Self::BattleshipApOverpenetration => {
                ("bb_ap_overpenetration", "%", Better::Higher, "the same hits that overpenetrate, doing little damage")
            }
            Self::CruiserHeDamaging => (
                "cruiser_he_damaging",
                "%",
                Better::Lower,
                "broadside HE hits of the cruiser HE shooter of its matchmaking at 5 to 10 km that penetrate, from armor_assessment",
            ),
            Self::BowOvermatch => (
                "bow_overmatch",
                "%",
                Better::Lower,
                "battleships of its matchmaking whose largest AP shell overmatches its thinnest bow plate above the waterline",
            ),
            Self::SternOvermatch => (
                "stern_overmatch",
                "%",
                Better::Lower,
                "battleships of its matchmaking whose largest AP shell overmatches its thinnest stern plate above the waterline",
            ),
        };

        Meta { key, unit, better, description }
    }

    /// The share as a plain statement when it is above half, so a majority
    /// reads as one whatever its rank: "67.21% of the battleships it meets
    /// overmatch its bow".
    fn majority_statement(self, value: f32) -> Option<String> {
        const HALF: f32 = 50.0;

        if value <= HALF {
            return None;
        }
        let share = number(value);
        match self {
            Self::BowOvermatch => Some(format!("{share}% of the battleships it meets overmatch its bow")),
            Self::SternOvermatch => Some(format!("{share}% of the battleships it meets overmatch its stern")),
            Self::BattleshipApCitadel => Some(format!("{share}% of these battleship AP hits detonate in its citadel")),
            _ => None,
        }
    }

    /// What an armor factor's value means for the ship, as its outlier line
    /// states it.
    fn outlier_statement(self, value: f32) -> Option<String> {
        let share = number(value);
        match self {
            Self::BattleshipApCitadel => Some(format!(
                "{share}% of broadside AP hits from the most common battleship caliber it meets, at 12 to 14 km, detonate in its citadel, a citadel hit being heavy damage taken"
            )),
            Self::BattleshipApOverpenetration => {
                Some(format!("{share}% of those battleship AP hits overpenetrate it, doing little damage"))
            }
            Self::CruiserHeDamaging => Some(format!(
                "{share}% of broadside cruiser HE hits at 5 to 10 km penetrate it, each regular damage taken"
            )),
            Self::BowOvermatch => Some(format!(
                "{share}% of the battleships it meets overmatch its bow, so their AP penetrates the bow at any angle"
            )),
            Self::SternOvermatch => Some(format!(
                "{share}% of the battleships it meets overmatch its stern, so their AP penetrates the stern at any angle"
            )),
            _ => None,
        }
    }

    /// Ranked only among ships of one tier: these are measured against the
    /// ships of each tier's matchmaking.
    fn per_tier(self) -> bool {
        matches!(
            self,
            Self::BattleshipApCitadel
                | Self::BattleshipApOverpenetration
                | Self::CruiserHeDamaging
                | Self::BowOvermatch
                | Self::SternOvermatch
        )
    }

    fn scope(self) -> RankScope {
        if self.per_tier() { RankScope::Tier } else { RankScope::Group }
    }

    /// A stock consumable value. Its leaders are left out of the tool answer
    /// when ships are named, to keep the answer short.
    fn is_consumable(self) -> bool {
        matches!(
            self,
            Self::RepairPartyHeal
                | Self::RepairPartyTotalHeal
                | Self::DamageControlReload
                | Self::RadarRange
                | Self::RadarDuration
                | Self::HydroRange
                | Self::TorpedoDetection
                | Self::SmokeLifetime
        )
    }

    /// `value` to the precision it is compared at: whole hit points, meters
    /// and degrees, else two decimals.
    fn rounded(self, value: f32) -> f32 {
        match self {
            Self::Health
            | Self::RepairPartyTotalHeal
            | Self::Alpha(_)
            | Self::DamagePerMinute(_)
            | Self::SecondaryDamagePerMinute
            | Self::TorpedoDamagePerMinute
            | Self::AntiAircraftRating
            | Self::AntiAircraftLongDps
            | Self::AntiAircraftFlak
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

    /// Main battery barrels bearing in the end arcs, for signals.
    arcs: Option<MountArcs>,

    /// Thinnest bow and stern plates above the waterline.
    bow_plate: Option<Millimeters>,
    stern_plate: Option<Millimeters>,

    matchups: Option<ShipMatchups>,
}

impl ShipScores {
    /// The unique upgrade `factor`'s value takes, if any.
    fn unique_for(&self, factor: Factor) -> Option<&str> {
        self.unique_upgrade.as_deref().filter(|_| self.unique_used.contains(&factor))
    }
}

/// Marks the values whose best-fit build takes a unique upgrade, so no other
/// value reads as taking one.
fn unique_marker(name: &str) -> String {
    format!("with unique upgrade {name}")
}

struct TorpedoScores {
    name: String,
    modules: Vec<String>,
    values: HashMap<TorpedoFactor, f32>,
    damage_by_distance: Option<String>,
    builds: Vec<String>,

    /// The ship's unique upgrade and the factors whose best-fit build takes it.
    unique_upgrade: Option<String>,
    unique_used: Vec<TorpedoFactor>,
}

impl TorpedoScores {
    /// The unique upgrade `factor`'s value takes, if any.
    fn unique_for(&self, factor: TorpedoFactor) -> Option<&str> {
        self.unique_upgrade.as_deref().filter(|_| self.unique_used.contains(&factor))
    }
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

impl GameData {
    /// The ships of one class and tier range with each factor of strength
    /// and its rank in the group.
    pub fn ship_comparison(&self, args: &ShipComparisonArgs) -> Result<ShipComparisonOutput, QueryError> {
        self.compare_group(args, Listing::Named)
    }

    /// [`GameData::ship_comparison`] with the entries `listing` selects.
    pub(crate) fn compare_group(
        &self,
        args: &ShipComparisonArgs,
        listing: Listing,
    ) -> Result<ShipComparisonOutput, QueryError> {
        const NOTES: &[&str] = &[
            "Values are on each ship's best-fit build for that factor, as ship_stats reports them. Few ships have a unique upgrade: a value takes one only where it says \"with unique upgrade <name>\", and an unmarked value takes none. Firing arcs use the module combination with the most barrels or tubes. Consumables and their factors (repair_party_*, damage_control_reload, radar_*, hydro_range, torpedo_detection, smoke_lifetime) and torpedo flood_coefficient are stock, repair_party_total_heal on the top hull's HP; a consumable factor takes the best option of any slot.",
            "rank r/n: position among the n ships of the group with the factor, 1 best, ties sharing a rank. pNN: the share of the other n - 1 ships the ship beats. Torpedo factors rank among every torpedo option of the group's ships; matchup rates among the peers armor_assessment compares with.",
            "outliers: values better or worse than at least 90% of the others, only where there are at least 4 others, each led by strength or weakness for the ship. Matchup outliers are from the ship's side as the target: a citadel hit is heavy damage taken, a penetration regular damage taken, an overpenetration little damage taken.",
            "bb_ap_*, cruiser_he_damaging, *_overmatch and matchups apply to cruisers and battleships only, and these factors rank among ships of the same tier: they are measured against the ships of each tier's matchmaking. Destroyers have no citadel: no armor judgment is made for them. Nominal plate thickness is not a factor: protection depends on every plate in front of the citadel.",
            REACTION_TIME_NOTE,
            "Damage rates assume every shell or torpedo hits; dispersion, sigma, range, flight time and concealment decide how much of it lands. battle_impact is a weighted judgment from these factors, not win-rate data; its formula and weights are given by the battle_impact tool.",
        ];
        const CRUISER_TYPE_NOTE: &str = "Cruisers are typed by their largest main battery caliber: light below 170 mm, heavy 170 to 260 mm, large above. Each cruiser value also ranks among the group's cruisers of its type, after the class rank, e.g. \"(rank 3/56, p96; heavy cruisers rank 9/24, p65)\": judge a cruiser against its own type.";

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
            nation: None,
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

        let mut scores: Vec<ShipScores> = parallel_map(&group, |ship| self.ship_scores(ship, reference_range))
            .into_iter()
            .collect::<Result<_, _>>()?;
        if matches!(class, ShipClass::Cruiser | ShipClass::Battleship) {
            self.add_protection(class, &group, &mut scores)?;
        }

        // The group's tiers as ranked, which ships named may widen.
        let shown_tiers = match (group.iter().map(|ship| ship.tier).min(), group.iter().map(|ship| ship.tier).max()) {
            (Some(min), Some(max)) => TierRange { min, max },
            _ => tiers,
        };
        let noun = class_plural(class);
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

        // With ships named their entries say where they stand; the group
        // needs only its extremes beside them.
        let shown = if named.is_empty() { 3 } else { 1 };
        // Cruisers rank among their own type as well: a light cruiser's
        // plating and guns say little about a heavy cruiser's.
        let cruiser_types: Vec<Option<CruiserType>> = group.iter().map(|ship| self.cruiser_type(ship)).collect();
        let first_type = cruiser_types.iter().flatten().next();
        let mixed_types = cruiser_types.iter().flatten().any(|cruiser| Some(cruiser) != first_type);
        // The nation keeps the model from guessing it from the name,
        // "premium" from reading a premium ship as the head of a line, and
        // the unique upgrade marks the only values that take one.
        let leader_name = |ship: &Ship, cruiser: Option<CruiserType>, unique: Option<&str>| {
            let unique = unique.map(unique_marker);
            let notes: Vec<&str> =
                [Some(ship.param.nation()), cruiser.map(CruiserType::label), ship.obtained_note(), unique.as_deref()]
                    .into_iter()
                    .flatten()
                    .collect();
            format!("{} ({})", ship.name, notes.join(", "))
        };
        let leaders = FactorLeaderTable(
            factors
                .iter()
                .map(|factor| {
                    let meta = factor.meta();
                    let values =
                        group.iter().zip(&scores).zip(&cruiser_types).filter_map(|((ship, score), cruiser)| {
                            let label = leader_name(ship, *cruiser, score.unique_for(*factor));
                            score.values.get(factor).map(|value| (label, *value))
                        });
                    (meta.key, factor_leaders(values.collect(), &meta, shown))
                })
                .collect(),
        );
        let torpedo_leaders = FactorLeaderTable(
            torpedo_factors
                .iter()
                .map(|factor| {
                    let meta = factor.meta();
                    let values = group.iter().zip(&scores).zip(&cruiser_types).flat_map(|((ship, score), cruiser)| {
                        score.torpedoes.iter().filter_map(move |torpedo| {
                            let label = leader_name(ship, *cruiser, torpedo.unique_for(*factor));
                            torpedo.values.get(factor).map(|value| (format!("{label} {}", torpedo.name), *value))
                        })
                    });
                    (meta.key, factor_leaders(values.collect(), &meta, shown))
                })
                .collect(),
        );

        let head_to_head = if named.len() < 2 {
            Vec::new()
        } else {
            let mut pairs = Vec::new();
            for (position, &first) in listed.iter().enumerate() {
                for &second in &listed[position + 1..] {
                    if group[first].class == group[second].class {
                        pairs.push(head_to_head(
                            &factors,
                            [group[first], group[second]],
                            [&scores[first], &scores[second]],
                        ));
                    }
                }
            }
            pairs
        };
        let mut entries: Vec<ComparedShip> = (0..group.len())
            .map(|index| {
                let ship = group[index];
                let score = &scores[index];
                let mut outliers = Vec::new();

                let mut values = Vec::new();
                let mut standings = Vec::new();
                for factor in &factors {
                    let Some(&value) = score.values.get(factor) else {
                        continue;
                    };
                    let meta = factor.meta();
                    let scope =
                        if factor.per_tier() { format!("tier {} {noun}", ship.tier.0) } else { group_label.clone() };
                    let peers = group
                        .iter()
                        .zip(&scores)
                        .filter(|(other, _)| !factor.per_tier() || other.tier == ship.tier)
                        .filter_map(|(_, other)| other.values.get(factor).copied());
                    let rank = GroupRank::new(value, meta.better, peers);
                    let unique = score.unique_for(*factor).map(|name| format!(" ({})", unique_marker(name)));
                    let unique = unique.unwrap_or_default();
                    if let Some((verdict, standing)) = rank.outlier_line(&scope, meta.better) {
                        let statement = match factor.outlier_statement(value) {
                            Some(statement) => format!("{statement} ({}{unique})", meta.key),
                            None => format!("{} {}{unique}", meta.key, meta.with_unit(value)),
                        };
                        outliers.push(format!("{verdict}: {statement}, {standing}"));
                    }
                    let own_type = cruiser_types[index].filter(|_| mixed_types);
                    let type_rank = own_type.map(|cruiser| {
                        let peers = group
                            .iter()
                            .zip(&scores)
                            .zip(&cruiser_types)
                            .filter(|((other, _), other_type)| {
                                **other_type == Some(cruiser) && (!factor.per_tier() || other.tier == ship.tier)
                            })
                            .filter_map(|((_, other), _)| other.values.get(factor).copied());
                        GroupRank::new(value, meta.better, peers)
                    });
                    let typed = own_type.zip(type_rank.as_ref());
                    let text = factor_text(*factor, value, &rank, &scope, typed);
                    values.push((meta.key, format!("{text}{unique}")));
                    standings.push(meta.standing(value, rank, factor.scope(), type_rank));
                }

                let torpedoes = score
                    .torpedoes
                    .iter()
                    .map(|torpedo| {
                        let mut values = Vec::new();
                        let mut standings = Vec::new();
                        for factor in &torpedo_factors {
                            let Some(&value) = torpedo.values.get(factor) else {
                                continue;
                            };
                            let meta = factor.meta();
                            let rank = GroupRank::new(value, meta.better, torpedo_pool(*factor));
                            let unique = torpedo.unique_for(*factor).map(|name| format!(" ({})", unique_marker(name)));
                            let unique = unique.unwrap_or_default();
                            if let Some((verdict, standing)) = rank.outlier_line(&torpedo_scope, meta.better) {
                                outliers.push(format!(
                                    "{verdict}: torpedo {} {} {}{unique}, {standing}",
                                    torpedo.name,
                                    meta.key,
                                    meta.with_unit(value)
                                ));
                            }
                            values.push((meta.key, format!("{}{unique}", rank.ranked(&number(value), &torpedo_scope))));
                            standings.push(meta.standing(value, rank, RankScope::Group, None));
                        }

                        TorpedoOption {
                            torpedo: torpedo.name.clone(),
                            modules: torpedo.modules.clone(),
                            values: FactorValues(values),
                            standings,
                            damage_by_distance: torpedo.damage_by_distance.clone(),
                            builds: torpedo.builds.clone(),
                        }
                    })
                    .collect();

                let matchups = match &score.matchups {
                    Some(matchups) => matchup_lines(matchups, class, &mut outliers, !named.is_empty()),
                    None => Vec::new(),
                };
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
                    category: ship.category,
                    cruiser_type: cruiser_types[index],
                    index: ship.param.index().to_owned(),
                    tier: ship.tier,
                    replicas: ship.replicas.clone(),
                    values: FactorValues(values),
                    standings,
                    outliers,
                    torpedoes,
                    matchups,
                    signals: signals(ship, score),
                    battle_impact: None,
                    unique_upgrade,
                    consumables: score.consumables.clone(),
                    special_abilities: score.special_abilities.clone(),
                }
            })
            .collect();
        let impacts = summaries(class, &entries);
        for (entry, impact) in entries.iter_mut().zip(impacts) {
            entry.battle_impact = impact;
        }
        let ships = match listing {
            Listing::Every => entries,
            Listing::Named => {
                let mut entries: Vec<Option<ComparedShip>> = entries.into_iter().map(Some).collect();
                listed.iter().filter_map(|index| entries[*index].take()).collect()
            }
        };

        Ok(ShipComparisonOutput {
            interpretations,
            group: group_label,
            ranked: group.len(),
            test: group.iter().filter(|ship| ship.status() == ShipStatus::Test).count(),
            torpedo_options: scores.iter().map(|score| score.torpedoes.len()).sum(),
            reference_range_km: reference_range,
            factors: factors.iter().map(|factor| factor.meta().legend()).collect(),
            torpedo_factors: torpedo_factors.iter().map(|factor| factor.meta().legend()).collect(),
            ship_names: group.iter().map(|ship| ship.name.clone()).collect(),
            leaders,
            torpedo_leaders,
            ships,
            head_to_head,
            notes: {
                let mut notes = NOTES.to_vec();
                if mixed_types {
                    notes.push(CRUISER_TYPE_NOTE);
                }
                notes
            },
        })
    }

    /// [`GameData::ship_comparison`] as the ship_comparison tool returns it:
    /// without ships named, the group summary alone, which keeps a whole
    /// class and tier within a small model's context; with ships named,
    /// their entries and the leaders, without the leaders of consumable
    /// factors, the list of ship names, and the torpedo sections when no
    /// named ship carries torpedoes.
    pub fn ship_comparison_summary(&self, args: &ShipComparisonArgs) -> Result<ShipComparisonOutput, QueryError> {
        let mut output = self.ship_comparison(args)?;
        if args.ships.as_ref().is_none_or(Vec::is_empty) {
            output.ships.clear();
        } else {
            output.ship_names.clear();
            let consumables: Vec<&str> = Factor::all()
                .into_iter()
                .filter(|factor| factor.is_consumable())
                .map(|factor| factor.meta().key)
                .collect();
            output.leaders.0.retain(|(key, _)| !consumables.contains(key));
            if output.ships.iter().all(|ship| ship.torpedoes.is_empty()) {
                output.torpedo_factors.clear();
                output.torpedo_leaders = FactorLeaderTable::default();
                output.notes.retain(|note| *note != REACTION_TIME_NOTE);
            }
        }

        Ok(output)
    }

    /// A cruiser's type by its largest main battery caliber over every gun
    /// module. `None` for other classes and for a cruiser without main
    /// battery shells.
    fn cruiser_type(&self, ship: &Ship) -> Option<CruiserType> {
        if ship.class != ShipClass::Cruiser {
            return None;
        }

        self.shells(ship)
            .iter()
            .filter(|shell| shell.battery == Battery::Main)
            .map(|shell| shell.caliber_mm)
            .max_by(|left, right| left.value().total_cmp(&right.value()))
            .map(CruiserType::from_caliber)
    }

    /// Every factor of one ship but protection, on its best-fit builds.
    fn ship_scores(&self, ship: &Ship, reference_range: Km) -> Result<ShipScores, QueryError> {
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
            arcs: None,
            bow_plate: None,
            stern_plate: None,
            matchups: None,
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
        let defense =
            |outcome: &Outcome| anti_air(&ship.param, planner.selection(outcome.selection), &outcome.bundle, None);
        let anti_aircraft: [(Factor, CardValue<'_>); 3] = [
            (Factor::AntiAircraftRating, &|outcome| Some(AaRating::from_defense(&defense(outcome)?).total.value())),
            (Factor::AntiAircraftLongDps, &|outcome| Some(defense(outcome)?.band(AuraBand::Long)?.dps().value())),
            (Factor::AntiAircraftFlak, &|outcome| {
                let flak = defense(outcome)?.flak?;
                Some(flak.auras.iter().map(|aura| aura.bursts.0 as f32 * aura.damage.value()).sum())
            }),
        ];
        for (factor, extract) in anti_aircraft {
            let rounded = |outcome: &Outcome| extract(outcome).map(|value| Amount(value.round()));
            let plan = StatPlan::new(factor.meta().key.to_owned(), factor.meta().better, &rounded);
            if let Some(builds) = planner.best(&plan)? {
                record(factor, builds.best.value.0, &builds.best.assumed);
            }
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

        let main_arcs = self.mount_arcs(ship, &planner, ComponentType::Artillery)?;
        if let Some(arcs) = main_arcs {
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

        let health = values.get(&Factor::Health).copied().map(Hp::from);
        values.extend(self.consumable_values(ship, stock_bundle, health));
        if matches!(ship.class, ShipClass::Cruiser | ShipClass::Battleship) {
            scores.bow_plate = self.zone_plate(ship, "Bow")?;
            scores.stern_plate = self.zone_plate(ship, "Stern")?;
        }

        scores.torpedoes = self.torpedo_scores(&planner, &rates, unique)?;
        scores.values = values;
        scores.unique_used = unique_used;
        scores.arcs = main_arcs;

        Ok(scores)
    }

    /// Each torpedo of every torpedo module, on its best-fit builds.
    fn torpedo_scores(
        &self,
        planner: &Planner<'_>,
        rates: &crate::rates::Rates,
        unique: Option<&Upgrade>,
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
            let unique_label = unique.map(Upgrade::build_label);
            let takes_unique = |assumed: &[String]| unique_label.as_ref().is_some_and(|label| assumed.contains(label));
            let mut unique_used = Vec::new();
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
                    if takes_unique(&builds.best.assumed) {
                        unique_used.push(factor);
                    }
                    values.insert(factor, best);
                }
            }

            let name = self.projectile_name(&param);
            if let Some(ammo) = rates.torpedo_damage.iter().find(|ammo| ammo.ammo == name) {
                let best = &ammo.rates.builds.best;
                values.insert(TorpedoFactor::DamagePerMinute, best.value.damage_per_minute.value().round());
                if takes_unique(&best.assumed) {
                    unique_used.push(TorpedoFactor::DamagePerMinute);
                }
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
                unique_upgrade: unique.map(|upgrade| upgrade.name.clone()),
                unique_used,
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
        let Some(params) =
            projectile.to_shell_info(shell.param.clone()).and_then(|info| ShellParams::from_shell_info(&info))
        else {
            return Ok(None);
        };

        Ok(solve_for_range(&params, range.to_meters()).map(|impact| impact.time_to_target.value()))
    }

    /// Thinnest plate of `zone` above the waterline; `None` without one.
    fn zone_plate(&self, ship: &Ship, zone: &str) -> Result<Option<Millimeters>, QueryError> {
        match zone_thickness(self, ship, zone) {
            Ok(plate) => Ok(Some(plate.thickness)),
            Err(QueryError::ZoneNotFound { .. }) => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// Matchups, battleship AP rates and bow and stern overmatch, from the
    /// peer table of `class` and each tier and that tier's matchmaking.
    fn add_protection(&self, class: ShipClass, group: &[&Ship], scores: &mut [ShipScores]) -> Result<(), QueryError> {
        let mut by_tier: BTreeMap<Tier, Vec<usize>> = BTreeMap::new();
        for (index, ship) in group.iter().enumerate() {
            by_tier.entry(ship.tier).or_default().push(index);
        }

        for (tier, indices) in by_tier {
            let ships: Vec<&Ship> = indices.iter().map(|index| group[*index]).collect();
            let matchups = self.matchups(class, tier, &ships)?;
            let calibers = self.battleship_calibers(tier);
            for (index, matchups) in indices.into_iter().zip(matchups) {
                let score = &mut scores[index];
                let overmatched = |plate: Option<Millimeters>| {
                    let plate = plate?;
                    let count = calibers.iter().filter(|caliber| is_overmatch(**caliber, plate)).count();
                    (!calibers.is_empty()).then(|| round2(count as f32 * 100.0 / calibers.len() as f32))
                };
                if let Some(share) = overmatched(score.bow_plate) {
                    score.values.insert(Factor::BowOvermatch, share);
                }
                if let Some(share) = overmatched(score.stern_plate) {
                    score.values.insert(Factor::SternOvermatch, share);
                }

                if let Some(battleship) = matchups.as_ref().and_then(battleship_broadside) {
                    if let Some(citadel) = &battleship.citadel {
                        score.values.insert(Factor::BattleshipApCitadel, citadel.value.value());
                    }
                    if let Some(overpenetration) = &battleship.overpenetration {
                        score.values.insert(Factor::BattleshipApOverpenetration, overpenetration.value.value());
                    }
                }
                if let Some(he) = matchups.as_ref().and_then(cruiser_he_broadside) {
                    score.values.insert(Factor::CruiserHeDamaging, he.damaging.value.value());
                }
                score.matchups = matchups;
            }
        }

        Ok(())
    }

    /// Largest main battery AP caliber of each battleship of `tier`'s
    /// matchmaking.
    fn battleship_calibers(&self, tier: Tier) -> Vec<Millimeters> {
        let filter = ShipFilter {
            tiers: Some(TierRange::around(tier)),
            class: Some(ShipClass::Battleship),
            nation: None,
            include_irregular: false,
            exclude_test: true,
            include_replicas: false,
        };

        self.ships
            .iter()
            .filter(|ship| filter.matches(ship))
            .filter_map(|ship| {
                let shells: Vec<Shell> =
                    self.shells(ship).into_iter().filter(|shell| shell.battery == Battery::Main).collect();
                largest_shell(&shells, Ammo::Ap).map(|shell| shell.caliber_mm)
            })
            .collect()
    }

    /// Stock consumable factors, each the best over every option of every
    /// slot. `health` is the maximum HP the Repair Party total is a share of.
    fn consumable_values(&self, ship: &Ship, bundle: &ModifierBundle, health: Option<Hp>) -> HashMap<Factor, f32> {
        let mut values: HashMap<Factor, f32> = HashMap::new();
        let Some(slots) = ship.param.vehicle().and_then(|vehicle| vehicle.abilities()) else {
            return values;
        };

        let mut keep = |factor: Factor, value: f32| {
            let value = factor.rounded(value);
            let kept = values.entry(factor).or_insert(value);
            if factor.meta().better.improves(value, *kept) {
                *kept = value;
            }
        };

        for (ability, variant) in slots.iter().flatten() {
            let Some(param) = self.provider().game_param_by_name(ability) else {
                continue;
            };
            let Some(category) = param.ability().and_then(|ability| ability.get_category(variant)) else {
                continue;
            };
            let stats = consumable_stats(self.consumable_name(ability, category), category, bundle);
            let km = |meters: Option<Meters>| meters.map(|meters| meters.to_km().value());
            match stats.kind.as_str() {
                "rls" => {
                    if let Some(range) = km(stats.ship_detection_m) {
                        keep(Factor::RadarRange, range);
                    }
                    if let Some(time) = stats.action_time_s {
                        keep(Factor::RadarDuration, time.value());
                    }
                }
                "sonar" => {
                    if let Some(range) = km(stats.ship_detection_m) {
                        keep(Factor::HydroRange, range);
                    }
                    if let Some(range) = km(stats.torpedo_detection_m) {
                        keep(Factor::TorpedoDetection, range);
                    }
                }
                "smokeGenerator" => {
                    if let Some(time) = stats.smoke_lifetime_s {
                        keep(Factor::SmokeLifetime, time.value());
                    }
                }
                "crashCrew" => keep(Factor::DamageControlReload, stats.reload_s.value()),
                "regenCrew" => {
                    if let (Some(rate), Some(time)) = (stats.heal_percent_per_s, stats.action_time_s) {
                        keep(Factor::RepairPartyHeal, rate.value() * time.value());
                    }
                    let total = health
                        .and_then(|health| repair_heal(category, bundle, health)?.per_battle)
                        .map(|heal| heal.hp.value());
                    if let Some(total) = total {
                        keep(Factor::RepairPartyTotalHeal, total);
                    }
                }
                // Other consumables are not compared.
                _ => {}
            }
        }

        values
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
                        let uses = match (stats.charges, stats.capacity_s) {
                            (Some(Charges::Limited(1)), _) => "1 charge".to_owned(),
                            (Some(Charges::Limited(count)), _) => format!("{count} charges"),
                            (Some(Charges::Unlimited), _) => "unlimited charges".to_owned(),
                            (None, Some(Capacity::Limited(capacity))) => format!("{} s of use", capacity.value()),
                            (None, Some(Capacity::Unlimited)) => "unlimited use".to_owned(),
                            (None, None) => {
                                return Some(format!("{} ({} s reload)", stats.name, stats.reload_s.value()));
                            }
                        };
                        Some(format!("{} ({uses}, {} s reload)", stats.name, stats.reload_s.value()))
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
    let salvo: f32 = mounts
        .iter()
        .filter_map(|(mount, barrels)| mount_salvo(mount.ammo(), *barrels, &shells))
        .map(|salvo| salvo.value())
        .sum();

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

/// Broadside rates against the most common battleship caliber at the
/// moderate band, where battleships fight cruisers and each other.
fn battleship_broadside(matchups: &ShipMatchups) -> Option<&Matchup> {
    matchups.matchups.iter().find(|matchup| {
        matchup.role == ShooterRole::BattleshipAp
            && matchup.band == RangeBand::Moderate
            && matchup.aspect == Aspect::Broadside
    })
}

/// Broadside rates against cruiser HE at the close band it fights at.
fn cruiser_he_broadside(matchups: &ShipMatchups) -> Option<&Matchup> {
    matchups.matchups.iter().find(|matchup| {
        matchup.role == ShooterRole::CruiserHe
            && matchup.band == fighting_band(ShooterRole::CruiserHe)
            && matchup.aspect == Aspect::Broadside
    })
}

/// The band a shooter group mostly fights at: battleships and heavy
/// cruisers at moderate range, the rest close.
fn fighting_band(role: ShooterRole) -> RangeBand {
    match role {
        ShooterRole::BattleshipAp | ShooterRole::HeavyCruiserAp => RangeBand::Moderate,
        ShooterRole::DestroyerAp | ShooterRole::LightCruiserAp | ShooterRole::CruiserHe | ShooterRole::Sap => {
            RangeBand::Close
        }
    }
}

/// The `shown` best and worst of `values`, named with their values. A
/// stable sort keeps tied values in the given order.
fn factor_leaders(mut values: Vec<(String, f32)>, meta: &Meta, shown: usize) -> FactorLeaders {
    values.sort_by(|(_, left), (_, right)| match meta.better {
        Better::Lower => left.total_cmp(right),
        Better::Higher => right.total_cmp(left),
    });
    let label = |(name, value): &(String, f32)| format!("{name} {}", meta.with_unit(*value));

    FactorLeaders {
        best: values.iter().take(shown).map(label).collect(),
        worst: values.iter().rev().take(shown).map(label).collect(),
    }
}

/// One line per shooter group at its fighting band, or the nearest band it
/// reaches, and outliers among the peers; lines only when `listed`.
fn matchup_lines(
    matchups: &ShipMatchups,
    class: ShipClass,
    outliers: &mut Vec<String>,
    listed: bool,
) -> Vec<MatchupLine> {
    let peers = format!("{} {}", tier_label(matchups.peer_tiers), class_plural(class));
    let mut shooters: Vec<&str> = Vec::new();
    for matchup in &matchups.matchups {
        if !shooters.contains(&matchup.shooter.as_str()) {
            shooters.push(&matchup.shooter);
        }
    }

    let mut lines = Vec::new();
    for shooter in shooters {
        let of_shooter: Vec<&Matchup> = matchups.matchups.iter().filter(|matchup| matchup.shooter == shooter).collect();
        let Some(first) = of_shooter.first() else {
            continue;
        };
        let wanted = fighting_band(first.role);
        let band = if of_shooter.iter().any(|matchup| matchup.band == wanted) { wanted } else { first.band };
        let at_band: Vec<&Matchup> = of_shooter.into_iter().filter(|matchup| matchup.band == band).collect();
        let Some(first) = at_band.first() else {
            continue;
        };
        let label = format!("{shooter} ({}), {}", first.shooter_ship, span(&first.ranges));

        let mut aspects = Vec::new();
        // One line per verdict and outcome: the aspects share the shooter and
        // peer group, so they are named once.
        let mut flagged: BTreeMap<(&'static str, MatchupOutcome), Vec<String>> = BTreeMap::new();
        for matchup in &at_band {
            let rates = [
                (MatchupOutcome::Citadel, matchup.citadel.as_ref()),
                (MatchupOutcome::Damaging, Some(&matchup.damaging)),
                (MatchupOutcome::Overpenetration, matchup.overpenetration.as_ref()),
            ];
            let mut parts = Vec::new();
            for (outcome, measured) in rates {
                let Some(Measured { value, peers: values }) = measured else {
                    continue;
                };
                let value = value.value();
                let better = outcome.better();
                let rank = GroupRank::new(value, better, values.iter().copied().chain([value]));
                parts.push(format!("{} {}", outcome.key(), rank.ranked(&format!("{}%", number(value)), &peers)));
                if let Some((verdict, standing)) = rank.outlier_line(&peers, better) {
                    flagged.entry((verdict, outcome)).or_default().push(format!(
                        "{}% of {} hits, {standing}",
                        number(value),
                        matchup.aspect.label()
                    ));
                }
            }
            aspects.push((aspect_key(matchup.aspect), parts.join(", ")));
        }
        // HE and SAP never overpenetrate; their damaging hits are penetrations.
        let penetrates_only = first.overpenetration.is_none();
        for ((verdict, outcome), cases) in flagged {
            outliers.push(format!(
                "{verdict}: {}'s {shooter} shells at {} {} on {}; {}.",
                first.shooter_ship,
                span(&first.ranges),
                outcome.result(penetrates_only),
                cases.join(" and "),
                outcome.meaning(penetrates_only),
            ));
        }

        if listed {
            lines.push(MatchupLine { shooter: label, aspects: FactorValues(aspects) });
        }
    }

    lines
}

/// An outcome of a shell hit that matchups rate, from the target's side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum MatchupOutcome {
    Citadel,

    /// Citadel and penetration hits together.
    Damaging,

    Overpenetration,
}

impl MatchupOutcome {
    fn key(self) -> &'static str {
        match self {
            Self::Citadel => "citadel",
            Self::Damaging => "damaging",
            Self::Overpenetration => "overpenetration",
        }
    }

    /// Which way the rate is better for the target.
    fn better(self) -> Better {
        match self {
            Self::Citadel | Self::Damaging => Better::Lower,
            Self::Overpenetration => Better::Higher,
        }
    }

    /// What the shells do, e.g. "detonate in its citadel".
    fn result(self, penetrates_only: bool) -> &'static str {
        match self {
            Self::Citadel => "detonate in its citadel",
            Self::Damaging if penetrates_only => "penetrate it",
            Self::Damaging => "detonate inside it (citadel or regular penetration)",
            Self::Overpenetration => "overpenetrate it",
        }
    }

    /// What the outcome means for the target.
    fn meaning(self, penetrates_only: bool) -> &'static str {
        match self {
            Self::Citadel => "a citadel hit is heavy damage taken",
            Self::Damaging if penetrates_only => "each such hit is regular damage taken",
            Self::Damaging => "each such hit is regular or heavy damage taken",
            Self::Overpenetration => "an overpenetration is little damage taken",
        }
    }
}

fn aspect_key(aspect: Aspect) -> &'static str {
    match aspect {
        Aspect::Broadside => "broadside",
        Aspect::Angled30 => "angled_30",
        Aspect::Angled45 => "angled_45",
    }
}

/// Two ships factor by factor: the factors each leads on, its largest leads,
/// and a verdict when one leads on far more factors.
fn head_to_head(factors: &[Factor], ships: [&Ship; 2], scores: [&ShipScores; 2]) -> HeadToHead {
    /// Largest leads listed per ship.
    const LEADS_SHOWN: usize = 3;
    /// A ship leading on at least this many times the factors the other
    /// leads on is better on paper.
    const LOPSIDED: usize = 2;

    struct Lead {
        key: &'static str,
        /// The gap as a share of the larger value.
        gap: f32,
        text: String,
    }

    let names = ships.map(|ship| ship.name.clone());
    let mut leads: [Vec<Lead>; 2] = [Vec::new(), Vec::new()];
    let mut tied = Vec::new();
    for factor in factors {
        let (Some(&first), Some(&second)) = (scores[0].values.get(factor), scores[1].values.get(factor)) else {
            continue;
        };
        let meta = factor.meta();
        // Values are already rounded to the precision they are compared at.
        if first == second {
            tied.push(meta.key);
            continue;
        }
        let first_leads = match meta.better {
            Better::Higher => first > second,
            Better::Lower => first < second,
        };
        let (leader, value, other) = if first_leads { (0, first, second) } else { (1, second, first) };
        let gap = (first - second).abs() / first.abs().max(second.abs());
        leads[leader].push(Lead {
            key: meta.key,
            gap,
            text: format!(
                "{} {} vs {} ({}% apart)",
                meta.key,
                number(value),
                meta.with_unit(other),
                (gap * 100.0).round()
            ),
        });
    }

    let compared = leads[0].len() + leads[1].len() + tied.len();
    let counts = [leads[0].len(), leads[1].len()];
    let keys = |side: &[Lead]| side.iter().map(|lead| lead.key).collect::<Vec<_>>();
    let first_better = keys(&leads[0]);
    let second_better = keys(&leads[1]);
    for side in &mut leads {
        side.sort_by(|left, right| right.gap.total_cmp(&left.gap));
    }

    let ties = if tied.is_empty() { String::new() } else { format!(", {} tied", tied.len()) };
    let split = format!(
        "{} is better on {} of {compared} compared factors, {} on {}{ties}",
        names[0], counts[0], names[1], counts[1]
    );
    let lopsided = (0..2).find(|&side| counts[side] > 0 && counts[side] >= LOPSIDED * counts[1 - side]);
    let verdict = match lopsided {
        // Sister ships tie on most factors; a lead on the few left is no verdict.
        _ if tied.len() * 2 > compared => {
            format!("Nearly the same on paper: they tie on {} of {compared} compared factors. {split}.", tied.len())
        }
        Some(side) => {
            let largest: Vec<&str> = leads[side].iter().take(LEADS_SHOWN).map(|lead| lead.key).collect();
            format!(
                "On paper {} is better: it leads on {} of {compared} compared factors, by the most on {}. {split}.",
                names[side],
                counts[side],
                largest.join(", ")
            )
        }
        None => format!(
            "Mixed: {split}; neither leads on twice as many factors as the other, so weigh the factors that matter for the question."
        ),
    };
    let largest = |side: &[Lead]| side.iter().take(LEADS_SHOWN).map(|lead| lead.text.clone()).collect();

    HeadToHead {
        first_largest_leads: largest(&leads[0]),
        second_largest_leads: largest(&leads[1]),
        ships: names,
        compared,
        first_better,
        second_better,
        tied,
        verdict,
    }
}

/// A ship factor's value with its ranks: "4.84 (rank 14/56, p73; heavy
/// cruisers rank 1/23, p91)". A majority share leads with the judgment, the
/// ranks after it: "weakness: 67.21% of the battleships it meets overmatch
/// its bow (a majority); still tied for the lowest share among tier 10
/// cruisers (rank 1/56, p89; ...)".
fn factor_text(
    factor: Factor,
    value: f32,
    rank: &GroupRank,
    scope: &str,
    typed: Option<(CruiserType, &GroupRank)>,
) -> String {
    let ranks = match typed {
        Some((cruiser, within)) => rank.position_text_with_type(scope, cruiser, within),
        None => rank.position_text(scope),
    };
    let Some(statement) = factor.majority_statement(value) else {
        return format!("{} ({ranks})", number(value));
    };

    // Majority factors are better lower: rank 1 holds the lowest share.
    let lowest = |within: &GroupRank, among: &str| {
        (within.position == 1).then(|| {
            let tie = if within.tied > 0 { "tied for the lowest" } else { "the lowest" };
            format!("still {tie} share among {among}")
        })
    };
    let best = lowest(rank, scope).or_else(|| typed.and_then(|(cruiser, within)| lowest(within, cruiser.plural())));
    let standing = match best {
        Some(best) => format!("{best} ({ranks})"),
        None => format!("for comparison, {ranks}"),
    };

    format!("weakness: {statement} (a majority); {standing}")
}

/// The play style a ship's arcs, plating and matchups point to, each with
/// the numbers behind it.
fn signals(ship: &Ship, score: &ShipScores) -> Vec<String> {
    /// Share of a battery bearing in an end arc below which it fights
    /// poorly in that direction.
    const HALF: f32 = 50.0;

    let mut signals = Vec::new();
    let value = |factor: Factor| score.values.get(&factor).copied();
    let matchmaking = TierRange::around(ship.tier);
    let battleships = format!("tier {}-{} battleships", matchmaking.min.0, matchmaking.max.0);
    let plate_clause = |share: Option<f32>, plate: Option<Millimeters>, end: &str| -> Option<(f32, String)> {
        let (share, plate) = (share?, plate?);
        // A majority reads as one, whatever its rank among cruisers.
        let who = if share > HALF {
            format!("most {battleships} ({}%)", number(share))
        } else {
            format!("{}% of {battleships}", number(share))
        };

        Some((share, format!("{who} overmatch its {} mm {end} plating", number(plate.value()))))
    };
    let stern = plate_clause(value(Factor::SternOvermatch), score.stern_plate, "stern");
    let bow = plate_clause(value(Factor::BowOvermatch), score.bow_plate, "bow");

    if let Some(arcs) = &score.arcs {
        let all = |angle: Option<u16>, end: &str| match angle {
            Some(angle) => format!("all bear from {angle} deg off the {end}"),
            None => format!("they never all bear toward the {end}"),
        };
        let astern = format!(
            "within 30 deg of the stern {} of {} main battery barrels bear and {}",
            arcs.stern,
            arcs.barrels,
            all(arcs.all_from_stern, "stern")
        );
        if arcs.share(arcs.stern) < HALF {
            match &stern {
                Some((share, clause))
                    if *share >= HALF && bow.as_ref().is_none_or(|(bow_share, _)| *bow_share < HALF) =>
                {
                    signals.push(format!("Bow-in island play suits it better than kiting: {astern}, and {clause}."))
                }
                _ => signals.push(format!("Kites poorly: {astern}.")),
            }
        } else {
            match &stern {
                Some((share, clause)) if *share >= HALF => {
                    signals.push(format!("Kites with its guns ({astern}), but {clause}."))
                }
                _ => signals.push(format!("Kites well: {astern}.")),
            }
        }

        let ahead = format!(
            "within 30 deg of the bow {} of {} main battery barrels bear and {}",
            arcs.bow,
            arcs.barrels,
            all(arcs.all_from_bow, "bow")
        );
        match &bow {
            Some((share, clause)) if *share >= HALF => {
                signals.push(format!("Bow-on it is exposed: {clause}; {ahead}."))
            }
            Some((_, clause)) if arcs.share(arcs.bow) >= HALF => {
                signals.push(format!("Bow-in suits it: {ahead}; only {clause}."))
            }
            _ => {}
        }
    }

    // Launchers that bear close to the bow allow a launch while closing or
    // retreating bow-on; launchers far from it force a broadside turn.
    if let Some(angle) = value(Factor::TorpedoLaunchFromBow) {
        if angle <= 10.0 {
            signals.push(format!(
                "Its torpedo launchers bear from {} deg off the bow, so it can launch while nearly bow-on.",
                number(angle)
            ));
        } else if angle >= 45.0 {
            signals.push(format!(
                "Its torpedo launchers bear only from {} deg off the bow, so a launch needs a wide turn toward broadside.",
                number(angle)
            ));
        }
    }

    if let Some(matchup) = score.matchups.as_ref().and_then(battleship_broadside) {
        let peers = score
            .matchups
            .as_ref()
            .map(|matchups| format!("{} {}", tier_label(matchups.peer_tiers), class_plural(ship.class)));
        let peers = peers.unwrap_or_default();
        let at = format!("{} ({}) at {}", matchup.shooter, matchup.shooter_ship, span(&matchup.ranges));
        if let Some(Measured { value, peers: values }) = &matchup.overpenetration {
            let rank = GroupRank::new(value.value(), Better::Higher, values.iter().copied().chain([value.value()]));
            if rank.others() >= OUTLIER_MIN_OTHERS && rank.worse * 4 >= rank.others() * 3 {
                signals.push(format!(
                    "Battleship AP mostly passes through it: {at}, {}% of broadside hits overpenetrate, more than {}% of {peers}. Overpenetration-prone plating is a strength against battleships.",
                    number(value.value()),
                    (rank.worse as f32 * 100.0 / rank.others() as f32).round()
                ));
            }
        }
        if let Some(Measured { value, peers: values }) = &matchup.citadel {
            let rank = GroupRank::new(value.value(), Better::Lower, values.iter().copied().chain([value.value()]));
            if rank.others() >= OUTLIER_MIN_OTHERS && rank.better * 4 >= rank.others() * 3 {
                signals.push(format!(
                    "Vulnerable to battleship AP broadside: {at}, {}% of broadside hits detonate in the citadel, more than {}% of {peers}. Angle against battleships.",
                    number(value.value()),
                    (rank.better as f32 * 100.0 / rank.others() as f32).round()
                ));
            }
        }
    }

    signals
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

/// "12-14 km", or "10 km" for one range.
fn span(ranges: &[Km]) -> String {
    match (ranges.first(), ranges.last()) {
        (Some(first), Some(last)) if first.value() != last.value() => {
            format!("{}-{} km", number(first.value()), number(last.value()))
        }
        (Some(first), _) => format!("{} km", number(first.value())),
        _ => String::new(),
    }
}

/// The key of every ship factor, as standings name them.
#[cfg(test)]
pub(crate) fn factor_keys() -> Vec<&'static str> {
    Factor::all().into_iter().map(|factor| factor.meta().key).collect()
}

/// Up to two decimals, without trailing zeros.
pub(crate) fn number(value: f32) -> String {
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
