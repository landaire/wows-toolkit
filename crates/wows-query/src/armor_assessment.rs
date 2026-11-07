//! How well a ship's armor protects it: armor traces from fixed aspects and
//! range bands, fired by shooters chosen from the game data for the ship's
//! matchmaking tiers, compared with ships of its class in its tier band.

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::HashSet;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::PoisonError;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use wows_core::units::Degrees;
use wows_core::units::Km;
use wows_core::units::Meters;
use wows_core::units::Millimeters;
use wows_core::units::Percent;
use wowsunpack::ballistics::ShellParams;
use wowsunpack::ballistics::simulate_shell_through_plates;
use wowsunpack::export::gltf_export::InteractiveArmorMesh;
use wowsunpack::game_params::ttx::armor_material::ArmorMaterial;

use crate::armor_trace::GunRequest;
use crate::armor_trace::RayOutcome;
use crate::armor_trace::TargetHull;
use crate::armor_trace::TracedRay;
use crate::armor_trace::Weapon;
use crate::armor_trace::WeaponModel;
use crate::armor_trace::ap_outcome;
use crate::armor_trace::bow_position;
use crate::armor_trace::cast_grid;
use crate::armor_trace::largest_shell;
use crate::armor_trace::line_of_fire;
use crate::armor_trace::plate_hits;
use crate::armor_trace::trace_ray;
use crate::citadel::CitadelGeometry;
use crate::citadel::FaceSummary;
use crate::citadel::Turtleback;
use crate::citadel::citadel_geometry;
use crate::citadel::has_citadel_plates;
use crate::data::BuildNumber;
use crate::data::GameData;
use crate::error::QueryError;
use crate::names::Interpretations;
use crate::raw_params::LAST_USED_MARKER;
use crate::raw_params::touch;
use crate::shells::Ammo;
use crate::shells::Battery;
use crate::shells::Shell;
use crate::ships::Ship;
use crate::ships::ShipClass;
use crate::ships::ShipFilter;
use crate::ships::Tier;
use crate::ships::TierRange;

/// Cache kind directory of peer comparison tables under a cache root, laid
/// out `<kind>/<build>/` like [`crate::raw_params::RAW_PARAMS_KIND`]. The
/// suffix is the table format.
pub const ARMOR_COMPARISONS_KIND: &str = "armor-comparisons-v1";

/// Revision of the tracing and outcome rules behind a peer table. Bump it
/// whenever a change alters traced rates, so stored tables are traced again.
const TABLE_REVISION: u32 = 4;

/// Distance between neighbouring rays of every assessment trace. Peers are
/// traced with the same spacing so their rates compare.
const TRACE_SPACING: Meters = Meters::new(1.0);

/// Ray spacing for the citadel's shape and exposure, as `citadel_profile`
/// uses by default.
const SIDE_SPACING: Meters = Meters::new(0.5);

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ArmorAssessmentArgs {
    /// Ship whose armor is assessed, e.g. "Khabarovsk".
    pub ship: String,

    /// Hull module, e.g. "Hull B". Defaults to the stock hull. Peers are compared on their stock hulls.
    pub hull: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ArmorAssessmentOutput {
    /// Names read as something they do not spell exactly, or as one of several ships sharing a name.
    #[serde(skip_serializing_if = "Interpretations::is_empty")]
    pub interpretations: Interpretations,

    pub ship: String,
    pub hull: String,

    /// For a class without a citadel: that citadel metrics do not apply.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub citadel_metrics: Option<&'static str>,

    /// Tiers the shooters were chosen from: the ship's matchmaking spread.
    pub shooter_tiers: TierRange,
    pub shooters: Vec<ShooterReport>,

    /// Outcomes per range band, shooter and aspect.
    pub bands: Vec<BandReport>,

    pub properties: ArmorProperties,
    pub comparison: Comparison,
    pub verdict: String,
    pub assumptions: Vec<&'static str>,
}

/// One shooter group: the most common caliber of one kind of gun in the
/// shooter tiers, fired by the ship whose shell is the median of that caliber.
#[derive(Debug, Clone, Serialize)]
pub struct ShooterReport {
    /// e.g. "battleship AP 406 mm".
    pub group: String,
    pub role: ShooterRole,
    pub ammo: Ammo,
    pub caliber_mm: Millimeters,

    /// Ships in the shooter tiers whose largest main battery shell of this
    /// ammo has this caliber, and how many ships of the role there are.
    pub ships_with_caliber: usize,
    pub ships_in_role: usize,

    /// The ship whose shell stands for the group, the median of the caliber
    /// by penetration.
    pub ship: String,

    /// English name of the shell.
    pub projectile: String,

    /// GameParams name of the shell.
    pub param: String,

    /// Main battery firing range with stock modules, no skills or upgrades.
    pub max_range_km: Km,

    /// AP: thickness after normalization that arms the fuse, the
    /// projectile's bulletDetonatorThreshold.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fuse_threshold_mm: Option<Millimeters>,

    /// HE and SAP: penetration of the first plate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub penetration_mm: Option<Millimeters>,
}

/// The kind of gun a shooter group stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ShooterRole {
    DestroyerAp,
    /// Cruiser AP of 155 mm and less, the London Naval Treaty limit of a
    /// light cruiser's guns.
    LightCruiserAp,
    HeavyCruiserAp,
    BattleshipAp,
    CruiserHe,
    Sap,
}

impl ShooterRole {
    /// The role of a ship's largest main battery shell of `ammo`.
    fn from_shell(class: ShipClass, ammo: Ammo, caliber: Millimeters) -> Option<Self> {
        const LIGHT_CRUISER_LIMIT: Millimeters = Millimeters::new(155.0);

        match (class, ammo) {
            (ShipClass::Destroyer, Ammo::Ap) => Some(Self::DestroyerAp),
            (ShipClass::Cruiser, Ammo::Ap) if caliber.value() <= LIGHT_CRUISER_LIMIT.value() => {
                Some(Self::LightCruiserAp)
            }
            (ShipClass::Cruiser, Ammo::Ap) => Some(Self::HeavyCruiserAp),
            (ShipClass::Battleship, Ammo::Ap) => Some(Self::BattleshipAp),
            (ShipClass::Cruiser, Ammo::He) => Some(Self::CruiserHe),
            (_, Ammo::Sap) => Some(Self::Sap),
            _ => None,
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::DestroyerAp => "destroyer AP",
            Self::LightCruiserAp => "light cruiser AP",
            Self::HeavyCruiserAp => "heavy cruiser AP",
            Self::BattleshipAp => "battleship AP",
            Self::CruiserHe => "cruiser HE",
            Self::Sap => "SAP",
        }
    }

    /// How many of the role's most common calibers are traced.
    fn caliber_count(self) -> usize {
        match self {
            Self::BattleshipAp => 2,
            _ => 1,
        }
    }
}

/// The target's heading relative to the line of fire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Aspect {
    #[serde(rename = "broadside")]
    Broadside,
    #[serde(rename = "angled_30")]
    Angled30,
    #[serde(rename = "angled_45")]
    Angled45,
}

impl Aspect {
    const ALL: [Self; 3] = [Self::Broadside, Self::Angled30, Self::Angled45];

    fn heading(self) -> Degrees {
        match self {
            Self::Broadside => Degrees::new(0.0),
            Self::Angled30 => Degrees::new(30.0),
            Self::Angled45 => Degrees::new(45.0),
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Broadside => "broadside",
            Self::Angled30 => "angled 30 deg",
            Self::Angled45 => "angled 45 deg",
        }
    }
}

/// Range bands, the same for every ship so assessments compare.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RangeBand {
    Close,
    Moderate,
    Long,
}

impl RangeBand {
    const ALL: [Self; 3] = [Self::Close, Self::Moderate, Self::Long];

    /// Ranges sampled in the band for a shooter of `class`.
    fn samples(self, class: ShipClass) -> &'static [Km] {
        const CLOSE: &[Km] = &[Km::new(5.0), Km::new(7.5), Km::new(10.0)];
        const MODERATE: &[Km] = &[Km::new(12.0), Km::new(14.0)];
        const LONG: &[Km] = &[Km::new(16.0), Km::new(18.0)];
        const LONG_BATTLESHIP: &[Km] = &[Km::new(16.0), Km::new(18.0), Km::new(20.0)];

        match (self, class) {
            (Self::Close, _) => CLOSE,
            (Self::Moderate, _) => MODERATE,
            (Self::Long, ShipClass::Battleship) => LONG_BATTLESHIP,
            (Self::Long, _) => LONG,
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Close => "close, 5 to 10 km",
            Self::Moderate => "moderate, 12 to 15 km",
            Self::Long => "long, 15 km and beyond (20 km for battleships only)",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct BandReport {
    pub band: RangeBand,
    pub label: &'static str,
    pub groups: Vec<GroupResult>,

    /// Shooter groups that reach no range sampled in this band.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub excluded: Vec<Excluded>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Excluded {
    pub group: String,
    pub max_range_km: Km,
}

#[derive(Debug, Clone, Serialize)]
pub struct GroupResult {
    #[serde(skip)]
    shooter: ShooterIndex,
    pub group: String,

    /// Sampled ranges within the shooter's reach. Rates are their mean, each
    /// range weighted equally.
    pub ranges_km: Vec<Km>,
    pub aspects: Vec<AspectResult>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AspectResult {
    pub aspect: Aspect,

    /// Modelled rays over all sampled ranges.
    pub rays: usize,

    #[serde(flatten)]
    pub rates: OutcomeRates,

    /// How the rates compare with the peers'. Absent without peers.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ranks: Option<Ranks>,
}

/// Shares of modelled rays by outcome, in percent.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct OutcomeRates {
    /// AP that detonates inside the citadel; HE or SAP that defeats citadel
    /// armor struck first.
    pub citadel_pct: Percent,

    /// AP that detonates inside the ship outside the citadel; HE or SAP that
    /// defeats the first plate.
    pub penetration_pct: Percent,

    /// AP that leaves the ship without detonating.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overpenetration_pct: Option<Percent>,

    pub ricochet_pct: Percent,

    /// Stopped by a plate, including AP that spends its velocity inside the
    /// armor, and HE or SAP that fails to defeat the first plate.
    pub shatter_pct: Percent,

    /// AP that detonates inside the ship because a plate armed its fuse, and
    /// would leave it without detonating had no plate armed the fuse.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fuse_armed_pct: Option<Percent>,
}

/// Shares of the peers with a higher and with a lower value, in whole
/// percent. The rest have the same value to 0.1.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Rank {
    pub higher_pct: Percent,
    pub lower_pct: Percent,

    /// Peers with a value to compare.
    #[serde(skip)]
    compared: usize,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct Ranks {
    /// Absent for a ship without citadel plates.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub citadel: Option<Rank>,

    /// Citadel and penetration together: hits that do damage behind armor.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub damaging: Option<Rank>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub fuse_armed: Option<Rank>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ArmorProperties {
    /// Absent when the armor model has no citadel plates.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub citadel: Option<CitadelPosition>,

    /// An armored deck sloping down behind a thicker belt to meet it near or
    /// below the waterline.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turtleback: Option<Turtleback>,

    /// The most common kind of citadel plate met above the waterline.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub above_waterline_face: Option<FaceSummary>,

    /// Per AP shooter group, the plates that arm the fuse of a shell that
    /// would otherwise leave the ship without detonating.
    pub fuse_arming: Vec<FuseArming>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CitadelPosition {
    /// Highest point of any citadel plate above the waterline.
    pub roof_above_waterline_m: Meters,

    /// Lowest point of any citadel plate, below the waterline when negative.
    pub floor_m: Meters,

    /// Share of the starboard side face, seen by horizontal rays, that is
    /// above the waterline. Absent when no horizontal ray meets the citadel.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub side_above_waterline_pct: Option<Percent>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub roof_rank: Option<Rank>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub side_above_waterline_rank: Option<Rank>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FuseArming {
    #[serde(skip)]
    shooter: ShooterIndex,
    pub group: String,
    pub fuse_threshold_mm: Millimeters,

    /// Rays over every band, aspect and sampled range where a plate arms the
    /// fuse of a shell that would otherwise leave the ship without detonating.
    pub rays: usize,

    /// Of all modelled rays of the group.
    pub share_pct: Percent,

    /// The plates that arm the fuse on those rays, most rays first.
    pub plates: Vec<ArmingPlateReport>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ArmingPlateReport {
    /// The game's name for the plate, led by its zone.
    pub plate: String,
    pub thickness_mm: Millimeters,
    pub rays: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct Comparison {
    pub class: ShipClass,
    pub tiers: TierRange,

    /// Released ships of the class in the tiers, the assessed ship left out,
    /// traced on their stock hulls against the same shooters.
    pub peers: usize,

    /// A few of them.
    pub examples: Vec<String>,

    /// Ships of the class in the tiers left out because they share the armor
    /// model of a listed ship, such as collaboration copies. Ships sharing
    /// the assessed ship's armor model are left out of `peers` too.
    #[serde(skip_serializing_if = "is_zero")]
    pub copies: usize,

    /// Ships of the class in the tiers left out because this game data holds
    /// no armor model for them.
    #[serde(skip_serializing_if = "is_zero")]
    pub without_armor_model: usize,

    /// Peers with citadel plates, and of them those with a turtleback.
    #[serde(skip_serializing_if = "is_zero")]
    pub with_citadel: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub with_turtleback: Option<usize>,
}

/// A peer table [`GameData::prepare_armor_comparisons`] could not trace.
#[derive(Debug)]
pub struct PreparationFailure {
    pub class: ShipClass,
    pub tier: Tier,
    pub error: QueryError,
}

/// One ship's rates against one shooter group at one band and aspect, each
/// with the peers' rates in the same condition.
#[derive(Debug, Clone)]
pub(crate) struct Matchup {
    /// e.g. "battleship AP 406 mm".
    pub(crate) shooter: String,

    /// English name of the ship that fires for the group.
    pub(crate) shooter_ship: String,
    pub(crate) role: ShooterRole,
    pub(crate) band: RangeBand,

    /// Sampled ranges of the band within the shooter's reach.
    pub(crate) ranges: Vec<Km>,
    pub(crate) aspect: Aspect,

    /// AP that detonates in the citadel. `None` for HE and SAP and without
    /// citadel plates; the peers are those with citadel plates.
    pub(crate) citadel: Option<Measured>,

    /// Citadel and penetration hits.
    pub(crate) damaging: Measured,

    /// AP that leaves the ship without detonating; `None` for HE and SAP.
    pub(crate) overpenetration: Option<Measured>,
}

/// A rate of one ship and the same rate of each peer.
#[derive(Debug, Clone)]
pub(crate) struct Measured {
    pub(crate) value: Percent,
    pub(crate) peers: Vec<f32>,
}

/// One ship's matchups against the shooters of its tier's peer table.
#[derive(Debug, Clone)]
pub(crate) struct ShipMatchups {
    /// Tiers of the peers the rates compare with.
    pub(crate) peer_tiers: TierRange,
    pub(crate) matchups: Vec<Matchup>,
}

/// The shooters an assessment traces, with the weapon at every range sampled.
struct ShooterSet {
    tiers: TierRange,
    shooters: Vec<Shooter>,
}

impl ShooterSet {
    /// The tracing rules and every projectile and range the set fires. A
    /// stored peer table traced otherwise is traced again.
    fn signature(&self) -> Vec<String> {
        let rules =
            format!("revision {TABLE_REVISION}, rays {} m, sides {} m", TRACE_SPACING.value(), SIDE_SPACING.value());
        let samples = self.shooters.iter().flat_map(|shooter| {
            shooter.samples.iter().map(|sample| format!("{} {}", shooter.report.param, sample.range))
        });

        std::iter::once(rules).chain(samples).collect()
    }
}

struct Shooter {
    report: ShooterReport,

    /// English name of the ship that fires for the group.
    name: String,
    samples: Vec<RangeSample>,
}

impl Shooter {
    fn ranges(&self, band: RangeBand) -> Vec<Km> {
        self.samples.iter().filter(|sample| sample.band == band).map(|sample| sample.range).collect()
    }
}

/// One range the shooter fires at, within its reach.
struct RangeSample {
    band: RangeBand,
    range: Km,
    weapon: Weapon,
}

/// A ship of the shooter tiers whose largest main battery shell of one ammo
/// gives it a role.
struct Candidate<'a> {
    ship: &'a Ship,
    shell: Shell,
}

/// Position of a shooter in its [`ShooterSet`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
struct ShooterIndex(usize);

/// A caliber to the whole millimeter, to group shells of one gun size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct WholeMm(u32);

impl WholeMm {
    fn from_caliber(caliber: Millimeters) -> Self {
        Self(caliber.value().round() as u32)
    }
}

/// A thickness to 0.1 mm, to key plates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct TenthsMm(u32);

impl TenthsMm {
    fn from_thickness(thickness: Millimeters) -> Self {
        Self((thickness.value() * 10.0).round() as u32)
    }

    fn to_mm(self) -> Millimeters {
        Millimeters::new(self.0 as f32 / 10.0)
    }
}

/// A hash of a ship's armor geometry and thicknesses: ships that share one
/// armor model hash alike. FNV-1a, so stored tables of one program version
/// compare with fingerprints computed later.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct ArmorFingerprint(u64);

impl ArmorFingerprint {
    pub(crate) fn from_meshes(meshes: &[InteractiveArmorMesh]) -> Self {
        const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
        const PRIME: u64 = 0x0000_0100_0000_01b3;

        let mut hash = OFFSET;
        let mut feed = |bytes: &[u8]| {
            for byte in bytes {
                hash ^= u64::from(*byte);
                hash = hash.wrapping_mul(PRIME);
            }
        };
        for mesh in meshes {
            for value in mesh.positions.iter().flatten().chain(mesh.transform.iter().flatten()) {
                feed(&value.to_bits().to_le_bytes());
            }
            for index in &mesh.indices {
                feed(&index.to_le_bytes());
            }
            for info in &mesh.triangle_info {
                feed(&[info.material_id]);
                feed(&info.thickness_mm.to_bits().to_le_bytes());
                feed(info.zone.as_bytes());
            }
        }

        Self(hash)
    }
}

/// Outcome counts of the rays of one trace.
#[derive(Debug, Clone, Copy, Default)]
struct Tally {
    rays: usize,
    citadel: usize,
    penetration: usize,
    overpenetration: usize,
    ricochet: usize,
    shatter: usize,
    fuse_armed: usize,
}

/// A band and aspect one shooter is traced at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
struct Condition {
    band: RangeBand,
    aspect: Aspect,
}

/// A plate that armed a fuse, by material and thickness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct ArmingPlate {
    material: ArmorMaterial,
    thickness: TenthsMm,
}

/// One shooter's traces of one ship.
#[derive(Default)]
struct ShooterTraces {
    /// One tally per sampled range of the band, in sample order.
    tallies: BTreeMap<Condition, Vec<Tally>>,
    arming: BTreeMap<ArmingPlate, usize>,
}

/// Rates over a condition's sampled ranges.
struct Summary {
    rays: usize,
    rates: OutcomeRates,
    damaging: Percent,
}

/// A peer's rates against one shooter at one band and aspect.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct PeerCondition {
    shooter: ShooterIndex,
    condition: Condition,
    citadel: Percent,
    damaging: Percent,
    overpenetration: Option<Percent>,
    fuse_armed: Option<Percent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PeerCitadel {
    roof_above_waterline: Meters,
    side_above_waterline: Option<Percent>,
    turtleback: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PeerMetrics {
    /// GameParams index.
    index: String,
    name: String,
    tier: Tier,
    fingerprint: ArmorFingerprint,
    conditions: Vec<PeerCondition>,
    citadel: Option<PeerCitadel>,
}

/// Every ship of one class in one tier band, traced against the shooters of
/// one tier.
#[derive(Debug, Serialize, Deserialize)]
struct PeerTable {
    signature: Vec<String>,
    peers: Vec<PeerMetrics>,
    without_armor_model: usize,

    /// Ships left out because they share a listed peer's armor model.
    copies: usize,
}

/// The ship class and tier a peer table is traced for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct ComparisonKey {
    class: ShipClass,
    tier: Tier,
}

/// One peer table, empty until loaded or traced.
type TableSlot = Arc<Mutex<Option<Arc<PeerTable>>>>;

/// Peer tables of one build, traced on first use and kept in memory and,
/// with a cache root, in `<cache_root>/<kind>/<build>/`.
pub(crate) struct ComparisonCache {
    dir: Option<PathBuf>,

    /// Each slot is locked while its table is loaded or traced, so a table
    /// is traced once.
    tables: Mutex<HashMap<ComparisonKey, TableSlot>>,

    /// Ships a table lacks, such as test ships, traced against its shooters,
    /// by table and GameParams index. Kept in memory only.
    untabled: Mutex<HashMap<(ComparisonKey, String), Arc<PeerMetrics>>>,

    /// Held while a table or untabled ships are traced, so a request and the
    /// background preparation together use at most half the processors.
    tracing: Mutex<()>,
}

impl ComparisonCache {
    pub(crate) fn new(cache_root: Option<&Path>, build: BuildNumber) -> Self {
        Self {
            dir: cache_root.map(|root| root.join(ARMOR_COMPARISONS_KIND).join(build.to_string())),
            tables: Mutex::new(HashMap::new()),
            untabled: Mutex::new(HashMap::new()),
            tracing: Mutex::new(()),
        }
    }

    /// The metrics of `ships`, none of them in `key`'s table, traced by
    /// `trace` on first use.
    fn untabled(
        &self,
        key: ComparisonKey,
        ships: &[(&Ship, ArmorFingerprint)],
        trace: impl Fn(&Ship, ArmorFingerprint) -> Result<PeerMetrics, QueryError> + Sync,
    ) -> Result<Vec<Arc<PeerMetrics>>, QueryError> {
        let cached = |ship: &Ship| {
            let untabled = self.untabled.lock().unwrap_or_else(PoisonError::into_inner);
            untabled.get(&(key, ship.param.index().to_owned())).cloned()
        };
        let mut found: Vec<Option<Arc<PeerMetrics>>> = ships.iter().map(|(ship, _)| cached(ship)).collect();
        if found.iter().any(Option::is_none) {
            let _tracing = self.tracing.lock().unwrap_or_else(PoisonError::into_inner);
            // Another request may have traced some while this one waited.
            for (slot, (ship, _)) in found.iter_mut().zip(ships) {
                if slot.is_none() {
                    *slot = cached(ship);
                }
            }
            let missing: Vec<usize> = (0..ships.len()).filter(|index| found[*index].is_none()).collect();
            let traced = parallel_map(&missing, |&index| trace(ships[index].0, ships[index].1));
            for (index, metrics) in missing.into_iter().zip(traced) {
                let metrics = Arc::new(metrics?);
                let mut untabled = self.untabled.lock().unwrap_or_else(PoisonError::into_inner);
                untabled.insert((key, ships[index].0.param.index().to_owned()), Arc::clone(&metrics));
                found[index] = Some(metrics);
            }
        }

        Ok(found.into_iter().flatten().collect())
    }

    fn table(
        &self,
        key: ComparisonKey,
        signature: &[String],
        trace: impl FnOnce() -> Result<PeerTable, QueryError>,
    ) -> Result<Arc<PeerTable>, QueryError> {
        let slot = {
            let mut tables = self.tables.lock().unwrap_or_else(PoisonError::into_inner);
            Arc::clone(tables.entry(key).or_default())
        };
        let mut slot = slot.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(table) = slot.as_ref() {
            self.touch();
            return Ok(Arc::clone(table));
        }

        let table = match self.load(key, signature) {
            Some(table) => table,
            None => {
                let table = {
                    let _tracing = self.tracing.lock().unwrap_or_else(PoisonError::into_inner);
                    trace()?
                };
                self.store(key, &table);
                table
            }
        };
        let table = Arc::new(table);
        *slot = Some(Arc::clone(&table));

        Ok(table)
    }

    /// Marks this build's stored tables as used now, for cache eviction.
    fn touch(&self) {
        if let Some(dir) = &self.dir {
            touch(&dir.join(LAST_USED_MARKER));
        }
    }

    fn path(&self, key: ComparisonKey) -> Option<PathBuf> {
        let dir = self.dir.as_ref()?;

        Some(dir.join(format!("{}-t{}.json", key.class.label(), key.tier.0)))
    }

    fn load(&self, key: ComparisonKey, signature: &[String]) -> Option<PeerTable> {
        let path = self.path(key)?;
        let file = File::open(&path).ok()?;
        let table: PeerTable = match serde_json::from_reader(BufReader::new(file)) {
            Ok(table) => table,
            Err(error) => {
                tracing::warn!(path = %path.display(), %error, "unreadable armor comparison table; tracing it again");
                return None;
            }
        };
        if table.signature != signature {
            return None;
        }
        self.touch();

        Some(table)
    }

    /// Writes through a temporary file in the same directory, so readers
    /// never see a partial table. A failure only costs the next process the
    /// same tracing.
    fn store(&self, key: ComparisonKey, table: &PeerTable) {
        let (Some(dir), Some(path)) = (self.dir.as_ref(), self.path(key)) else {
            return;
        };
        let staging = path.with_extension(format!("json.tmp-{}", std::process::id()));
        let written = std::fs::create_dir_all(dir)
            .and_then(|()| serde_json::to_vec(table).map_err(std::io::Error::other))
            .and_then(|bytes| std::fs::write(&staging, bytes))
            .and_then(|()| std::fs::rename(&staging, &path));
        match written {
            Ok(()) => self.touch(),
            Err(error) => {
                let _ = std::fs::remove_file(&staging);
                tracing::warn!(path = %path.display(), %error, "failed to store armor comparison table");
            }
        }
    }
}

impl GameData {
    /// How often a ship's armor yields citadels, penetrations,
    /// overpenetrations, ricochets and shatters against shooters of its
    /// matchmaking tiers, in fixed range bands and aspects, with its citadel
    /// properties and plates that arm fuses, ranked among its class in its
    /// tier band.
    pub fn armor_assessment(&self, args: &ArmorAssessmentArgs) -> Result<ArmorAssessmentOutput, QueryError> {
        let mut interpretations = Interpretations::default();
        let ship = self.resolve_ship(&args.ship, &mut interpretations)?;
        let TargetHull { param: hull_param, name: hull_name } = self.target_hull(ship, args.hull.as_deref())?;
        let meshes = self.target_meshes(ship, &hull_param)?;

        let set = self.shooter_set(ship.tier)?;
        let traces = trace_ship(&meshes, &set)?;
        let geometry = side_geometry(&meshes)?;

        let key = ComparisonKey { class: ship.class, tier: ship.tier };
        let table = self.armor_comparisons.table(key, &set.signature(), || self.trace_peers(key, &set))?;
        let fingerprint = ArmorFingerprint::from_meshes(&meshes);
        let peers: Vec<&PeerMetrics> = table
            .peers
            .iter()
            .filter(|peer| peer.index != ship.param.index() && peer.fingerprint != fingerprint)
            .collect();

        let comparison = comparison(key, ship, &peers, table.copies, table.without_armor_model);
        let bands = band_reports(&set, &traces, geometry.is_some(), &peers);
        let materials = meshes.iter().flat_map(|mesh| mesh.triangle_info.iter().map(|info| info.material));
        let names = self.plate_names(ship, materials);
        let properties = ArmorProperties {
            citadel: geometry.as_ref().map(|geometry| citadel_position(geometry, &peers)),
            turtleback: geometry.as_ref().and_then(|geometry| geometry.shape.turtleback.clone()),
            above_waterline_face: geometry.as_ref().and_then(|geometry| geometry.shape.above_waterline.clone()),
            fuse_arming: fuse_arming(&set, &traces, |material| names.of(material)),
        };
        let verdict = verdict(ship, &set, &bands, &properties, &comparison);

        Ok(ArmorAssessmentOutput {
            interpretations,
            ship: ship.label(),
            hull: hull_name,
            citadel_metrics: citadel_metrics(ship.class),
            shooter_tiers: set.tiers,
            shooters: set.shooters.iter().map(|shooter| shooter.report.clone()).collect(),
            bands,
            properties,
            comparison,
            verdict,
            assumptions: vec![
                "Shooter groups are the most common caliber of destroyer AP, light cruiser AP (155 mm and less), heavy cruiser AP, battleship AP (two calibers), cruiser HE and SAP among released ships of the shooter tiers, each counted by its largest main battery shell of that ammo; the ship whose shell is the median of that caliber by penetration fires for the group, a tech tree ship before its copies.",
                "Aspects are broadside and 30 and 45 deg angled, bow toward the shooter. Range bands are close (5, 7.5 and 10 km), moderate (12 and 14 km) and long (16 and 18 km, and 20 km for battleship shooters). A shooter fires only at ranges within its stock main battery range; a band's rates are the mean over the ranges it reaches, each weighted equally.",
                "Rays are 1 m apart and each stands for an equal area of the target seen along the line of fire; rates are shares of rays that strike armor above the waterline, not weighted by dispersion or aim.",
                "Each ray is traced as armor_trace traces it: AP through every plate with the armor viewer's simulation, HE and SAP against the first plate, ricochets between the ricochet start and always-ricochet angles treated as no ricochet.",
                "A fuse is armed by a plate whose thickness after normalization is at least the shell's bulletDetonatorThreshold. Fuse-armed rays are AP that detonates inside the ship and, traced again with no plate arming the fuse, leaves the ship without detonating.",
                "Peers are released ships of the same class within one tier, traced on their stock hulls against the same shooters and ranges; ships sharing one armor model count once. A rank gives the shares of peers with a higher and with a lower value, to 0.1.",
            ],
        })
    }

    /// Traces the peer table of every class and tier with released ships,
    /// highest tier first, so later assessments find them ready. With a
    /// cache root, the tables are stored for later processes of this build.
    /// A table that fails is reported and the rest are still traced.
    pub fn prepare_armor_comparisons(&self) -> Vec<PreparationFailure> {
        let filter = ShipFilter { exclude_test: true, ..ShipFilter::default() };
        let mut keys: Vec<ComparisonKey> = self
            .ships
            .iter()
            .filter(|ship| filter.matches(ship))
            .map(|ship| ComparisonKey { class: ship.class, tier: ship.tier })
            .collect();
        keys.sort_by(|left, right| right.tier.cmp(&left.tier).then_with(|| left.class.cmp(&right.class)));
        keys.dedup();

        let mut failures = Vec::new();
        for key in keys {
            let traced = self
                .shooter_set(key.tier)
                .and_then(|set| self.armor_comparisons.table(key, &set.signature(), || self.trace_peers(key, &set)));
            if let Err(error) = traced {
                failures.push(PreparationFailure { class: key.class, tier: key.tier, error });
            }
        }

        failures
    }

    fn shooter_set(&self, tier: Tier) -> Result<ShooterSet, QueryError> {
        let tiers = TierRange::around(tier);
        let filter = ShipFilter {
            tiers: Some(tiers),
            class: None,
            nation: None,
            include_irregular: false,
            exclude_test: true,
            include_replicas: false,
        };

        let mut candidates: BTreeMap<ShooterRole, Vec<Candidate>> = BTreeMap::new();
        for ship in self.ships.iter().filter(|ship| filter.matches(ship)) {
            let shells: Vec<Shell> =
                self.shells(ship).into_iter().filter(|shell| shell.battery == Battery::Main).collect();
            for ammo in [Ammo::Ap, Ammo::He, Ammo::Sap] {
                let Some(shell) = largest_shell(&shells, ammo) else {
                    continue;
                };
                let Some(role) = ShooterRole::from_shell(ship.class, ammo, shell.caliber_mm) else {
                    continue;
                };
                candidates.entry(role).or_default().push(Candidate { ship, shell: shell.clone() });
            }
        }

        let mut shooters = Vec::new();
        for (role, candidates) in &candidates {
            for caliber in common_calibers(candidates, role.caliber_count()) {
                if let Some(shooter) = self.shooter(*role, caliber, candidates)? {
                    shooters.push(shooter);
                }
            }
        }

        Ok(ShooterSet { tiers, shooters })
    }

    /// The group of `role` at `caliber`, fired by the ship whose shell is the
    /// median of that caliber by penetration. `None` when no shell of the
    /// caliber has the data a ballistic solution needs.
    fn shooter(
        &self,
        role: ShooterRole,
        caliber: WholeMm,
        candidates: &[Candidate],
    ) -> Result<Option<Shooter>, QueryError> {
        let mut ranked: Vec<(&Candidate, Millimeters)> = Vec::new();
        for candidate in
            candidates.iter().filter(|candidate| WholeMm::from_caliber(candidate.shell.caliber_mm) == caliber)
        {
            let penetration = match candidate.shell.ammo {
                Ammo::Ap => {
                    let projectile = self.projectile(&candidate.shell.param)?;
                    ShellParams::from_shell_info(&projectile.to_shell_info(candidate.shell.param.clone()))
                        .map(|params| params.raw_penetration(params.muzzle_velocity))
                }
                Ammo::He | Ammo::Sap => candidate.shell.penetration_mm,
            };
            if let Some(penetration) = penetration {
                ranked.push((candidate, penetration));
            }
        }
        let ships_with_caliber = ranked.len();
        ranked.sort_by(|left, right| {
            same_shell_order(left, right)
                .then_with(|| left.0.ship.rank().cmp(&right.0.ship.rank()))
                .then_with(|| left.0.ship.name.cmp(&right.0.ship.name))
        });
        let middle = ranked.len() / 2;
        let Some(median) = ranked.get(middle) else {
            return Ok(None);
        };

        // Of the ships firing the median shell, such as a ship and its
        // collaboration copies, the first by group (tech tree first) fires
        // for the group.
        let first = ranked[..middle]
            .iter()
            .rposition(|entry| same_shell_order(entry, median).is_ne())
            .map_or(0, |index| index + 1);
        let chosen = ranked[first].0;

        let ship = chosen.ship;
        let ammo = chosen.shell.ammo;
        let card = self.card(ship, &self.stock_loadout())?;
        let Some(range) = card.stats.artillery.as_ref().and_then(|artillery| artillery.range) else {
            return Err(QueryError::WeaponNotFound {
                ship: ship.name.clone(),
                weapon: "main battery range".to_owned(),
            });
        };
        let max_range = Km::new(range.value());

        let mut samples = Vec::new();
        for band in RangeBand::ALL {
            for &range in band.samples(ship.class) {
                if range.value() > max_range.value() {
                    continue;
                }
                let request = GunRequest { battery: Battery::Main, ammo, range_km: Some(range), ifhe: false };
                samples.push(RangeSample { band, range, weapon: self.gun_weapon(ship, &request)? });
            }
        }
        let fuse_threshold = samples.first().and_then(|sample| match &sample.weapon.model {
            WeaponModel::ArmorPiercing { params, .. } => Some(params.fuse_threshold),
            WeaponModel::FirstPlate { .. } => None,
        });

        let report = ShooterReport {
            group: format!("{} {} mm", role.label(), caliber.0),
            role,
            ammo,
            caliber_mm: chosen.shell.caliber_mm,
            ships_with_caliber,
            ships_in_role: candidates.len(),
            ship: ship.label(),
            projectile: chosen.shell.name.clone(),
            param: chosen.shell.param.clone(),
            // Two decimals, so a range just short of a sample does not read
            // as reaching it.
            max_range_km: Km::new((max_range.value() * 100.0).round() / 100.0),
            fuse_threshold_mm: fuse_threshold,
            penetration_mm: match ammo {
                Ammo::Ap => None,
                Ammo::He | Ammo::Sap => chosen.shell.penetration_mm,
            },
        };

        Ok(Some(Shooter { report, name: ship.name.clone(), samples }))
    }

    /// Traces every released ship of `key`'s class in its tier band. Of
    /// ships that share one armor model, such as collaboration copies, the
    /// first by group (tech tree first) and name stands for them.
    fn trace_peers(&self, key: ComparisonKey, set: &ShooterSet) -> Result<PeerTable, QueryError> {
        let filter = ShipFilter {
            tiers: Some(TierRange::adjacent(key.tier)),
            class: Some(key.class),
            nation: None,
            include_irregular: false,
            exclude_test: true,
            include_replicas: false,
        };
        let mut ships: Vec<&Ship> = self.ships.iter().filter(|ship| filter.matches(ship)).collect();
        ships.sort_by(|left, right| left.rank().cmp(&right.rank()).then_with(|| left.name.cmp(&right.name)));

        let fingerprints = parallel_map(&ships, |ship| {
            self.stock_meshes(ship).map(|meshes| meshes.as_deref().map(ArmorFingerprint::from_meshes))
        });
        let mut seen: HashSet<ArmorFingerprint> = HashSet::new();
        let mut unique: Vec<(&Ship, ArmorFingerprint)> = Vec::new();
        let mut without_armor_model = 0;
        let mut copies = 0;
        for (ship, fingerprint) in ships.into_iter().zip(fingerprints) {
            match fingerprint? {
                None => without_armor_model += 1,
                Some(fingerprint) if seen.insert(fingerprint) => unique.push((ship, fingerprint)),
                Some(_) => copies += 1,
            }
        }

        let peers = parallel_map(&unique, |&(ship, fingerprint)| self.peer_metrics(ship, fingerprint, set))
            .into_iter()
            .collect::<Result<Vec<_>, _>>()?;

        Ok(PeerTable { signature: set.signature(), peers, without_armor_model, copies })
    }

    /// The armor of `ship`'s stock hull; `None` when the game data holds no
    /// armor model for it.
    pub(crate) fn stock_meshes(&self, ship: &Ship) -> Result<Option<Vec<InteractiveArmorMesh>>, QueryError> {
        match self.target_hull(ship, None).and_then(|hull| self.target_meshes(ship, &hull.param)) {
            Ok(meshes) => Ok(Some(meshes)),
            Err(QueryError::ArmorMissing { .. }) => Ok(None),
            Err(error) => Err(error),
        }
    }

    fn peer_metrics(
        &self,
        ship: &Ship,
        fingerprint: ArmorFingerprint,
        set: &ShooterSet,
    ) -> Result<PeerMetrics, QueryError> {
        let Some(meshes) = self.stock_meshes(ship)? else {
            return Err(QueryError::ArmorMissing { ship: ship.name.clone() });
        };

        let traces = trace_ship(&meshes, set)?;
        let geometry = side_geometry(&meshes)?;

        let mut conditions = Vec::new();
        for (index, shooter) in traces.iter().enumerate() {
            for (condition, tallies) in &shooter.tallies {
                let Some(summary) = summarize(tallies, set.shooters[index].report.ammo) else {
                    continue;
                };
                conditions.push(PeerCondition {
                    shooter: ShooterIndex(index),
                    condition: *condition,
                    citadel: summary.rates.citadel_pct,
                    damaging: summary.damaging,
                    overpenetration: summary.rates.overpenetration_pct,
                    fuse_armed: summary.rates.fuse_armed_pct,
                });
            }
        }

        Ok(PeerMetrics {
            index: ship.param.index().to_owned(),
            name: ship.name.clone(),
            tier: ship.tier,
            fingerprint,
            conditions,
            citadel: geometry.map(|geometry| PeerCitadel {
                roof_above_waterline: geometry.exposure.roof_above_waterline_m,
                side_above_waterline: side_above_waterline(&geometry),
                turtleback: geometry.shape.turtleback.is_some(),
            }),
        })
    }

    /// Each of `ships`, all of `class` and `tier`, against every shooter
    /// group of that tier's peer table at each band and aspect, with the
    /// table's peers' rates. A ship the table lacks, such as a test ship, is
    /// traced against the same shooters once per process. `None` for a ship
    /// without an armor model.
    pub(crate) fn matchups(
        &self,
        class: ShipClass,
        tier: Tier,
        ships: &[&Ship],
    ) -> Result<Vec<Option<ShipMatchups>>, QueryError> {
        /// Where a ship's metrics come from.
        enum Located {
            /// Index into the table's peers.
            Listed(usize),

            /// Index into the untabled ships.
            Untabled(usize),
            WithoutArmorModel,
        }

        let set = self.shooter_set(tier)?;
        let key = ComparisonKey { class, tier };
        let table = self.armor_comparisons.table(key, &set.signature(), || self.trace_peers(key, &set))?;

        let found = parallel_map(ships, |ship| -> Result<Result<usize, Option<ArmorFingerprint>>, QueryError> {
            if let Some(index) = table.peers.iter().position(|peer| peer.index == ship.param.index()) {
                return Ok(Ok(index));
            }
            let Some(meshes) = self.stock_meshes(ship)? else {
                return Ok(Err(None));
            };

            // Ships sharing a listed peer's armor model, such as collaboration
            // copies, are listed once in the table.
            let fingerprint = ArmorFingerprint::from_meshes(&meshes);
            Ok(table.peers.iter().position(|peer| peer.fingerprint == fingerprint).ok_or(Some(fingerprint)))
        });
        let mut untabled: Vec<(&Ship, ArmorFingerprint)> = Vec::new();
        let mut located = Vec::with_capacity(ships.len());
        for (ship, found) in ships.iter().zip(found) {
            located.push(match found? {
                Ok(index) => Located::Listed(index),
                Err(Some(fingerprint)) => {
                    untabled.push((ship, fingerprint));
                    Located::Untabled(untabled.len() - 1)
                }
                Err(None) => Located::WithoutArmorModel,
            });
        }
        let traced = self
            .armor_comparisons
            .untabled(key, &untabled, |ship, fingerprint| self.peer_metrics(ship, fingerprint, &set))?;

        let peer_tiers = TierRange::adjacent(tier);
        Ok(located
            .into_iter()
            .map(|located| {
                let metrics: &PeerMetrics = match located {
                    Located::Listed(index) => &table.peers[index],
                    Located::Untabled(index) => &traced[index],
                    Located::WithoutArmorModel => return None,
                };
                let peers: Vec<&PeerMetrics> = table
                    .peers
                    .iter()
                    .filter(|peer| peer.index != metrics.index && peer.fingerprint != metrics.fingerprint)
                    .collect();

                Some(ShipMatchups { peer_tiers, matchups: matchups_of(metrics, &peers, &set) })
            })
            .collect())
    }
}

/// `map` over `items` on half the processors, as a chat host also serves
/// other requests, in item order.
pub(crate) fn parallel_map<T, R>(items: &[T], map: impl Fn(&T) -> R + Sync) -> Vec<R>
where
    T: Sync,
    R: Send,
{
    let workers = std::thread::available_parallelism().map_or(1, |count| (count.get() / 2).max(1)).min(items.len());
    let next = AtomicUsize::new(0);
    let mut done: Vec<(usize, R)> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..workers)
            .map(|_| {
                scope.spawn(|| {
                    let mut done = Vec::new();
                    loop {
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        let Some(item) = items.get(index) else {
                            break;
                        };
                        done.push((index, map(item)));
                    }
                    done
                })
            })
            .collect();

        handles
            .into_iter()
            .flat_map(|handle| handle.join().unwrap_or_else(|panic| std::panic::resume_unwind(panic)))
            .collect()
    });
    done.sort_by_key(|(index, _)| *index);

    done.into_iter().map(|(_, result)| result).collect()
}

/// Orders ranked shells by penetration, then muzzle velocity and alpha
/// damage; shells equal in all three are the same shell.
fn same_shell_order(left: &(&Candidate, Millimeters), right: &(&Candidate, Millimeters)) -> std::cmp::Ordering {
    let optional = |left: Option<f32>, right: Option<f32>| match (left, right) {
        (Some(left), Some(right)) => left.total_cmp(&right),
        _ => left.is_some().cmp(&right.is_some()),
    };

    left.1
        .value()
        .total_cmp(&right.1.value())
        .then_with(|| {
            optional(
                left.0.shell.muzzle_velocity_mps.map(|speed| speed.value()),
                right.0.shell.muzzle_velocity_mps.map(|speed| speed.value()),
            )
        })
        .then_with(|| optional(left.0.shell.alpha_damage, right.0.shell.alpha_damage))
}

/// The `count` calibers most ships fire, most first; equal counts put the
/// larger caliber first.
fn common_calibers(candidates: &[Candidate], count: usize) -> Vec<WholeMm> {
    let mut ships: BTreeMap<WholeMm, usize> = BTreeMap::new();
    for candidate in candidates {
        *ships.entry(WholeMm::from_caliber(candidate.shell.caliber_mm)).or_default() += 1;
    }
    let mut calibers: Vec<(WholeMm, usize)> = ships.into_iter().collect();
    calibers.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| right.0.cmp(&left.0)));

    calibers.into_iter().take(count).map(|(caliber, _)| caliber).collect()
}

/// Traces `meshes` against every shooter of `set`, at each sampled range and
/// aspect.
fn trace_ship(meshes: &[InteractiveArmorMesh], set: &ShooterSet) -> Result<Vec<ShooterTraces>, QueryError> {
    let mut traces = Vec::with_capacity(set.shooters.len());
    for shooter in &set.shooters {
        let mut shooter_traces = ShooterTraces::default();
        for sample in &shooter.samples {
            for aspect in Aspect::ALL {
                let direction = line_of_fire(aspect.heading(), sample.weapon.arrival.fall_angle_deg);
                let grid = cast_grid(meshes, direction, TRACE_SPACING)?;
                let mut tally = Tally::default();
                for ray in grid.rays.into_iter().filter(|ray| !ray.underwater) {
                    let traced = trace_ray(&sample.weapon.model, ray.plates);
                    count(&mut tally, traced.outcome);
                    if let Some(plate) = arming_plate(&sample.weapon.model, &traced) {
                        tally.fuse_armed += 1;
                        *shooter_traces.arming.entry(plate).or_default() += 1;
                    }
                }
                let condition = Condition { band: sample.band, aspect };
                shooter_traces.tallies.entry(condition).or_default().push(tally);
            }
        }
        traces.push(shooter_traces);
    }

    Ok(traces)
}

fn count(tally: &mut Tally, outcome: RayOutcome) {
    tally.rays += 1;
    let counter = match outcome {
        RayOutcome::Citadel => &mut tally.citadel,
        RayOutcome::Penetration => &mut tally.penetration,
        RayOutcome::Overpenetration => &mut tally.overpenetration,
        RayOutcome::Ricochet => &mut tally.ricochet,
        RayOutcome::Shatter | RayOutcome::Stopped => &mut tally.shatter,
    };
    *counter += 1;
}

/// The plate that armed the fuse of an AP shell that detonated inside the
/// ship, when the shell would have left the ship without detonating had
/// that plate not armed it: no later plate arms the fuse in time.
fn arming_plate(model: &WeaponModel, ray: &TracedRay) -> Option<ArmingPlate> {
    let WeaponModel::ArmorPiercing { params, .. } = model else {
        return None;
    };
    if !matches!(ray.outcome, RayOutcome::Citadel | RayOutcome::Penetration) {
        return None;
    }
    let sim = ray.sim.as_ref()?;
    let armed_at = sim.detonation?.armed_at.value();
    let plate = ray.plates.get(armed_at)?;

    // Arming does not slow the shell, so it leaves the arming plate as it
    // did and meets the plates behind it with an unarmed fuse.
    let velocity = sim.plates.get(armed_at)?.velocity_after?;
    let behind = &ray.plates[armed_at + 1..];
    let unarmed = simulate_shell_through_plates(params, velocity, &plate_hits(behind), false);
    let zones: Vec<&str> = behind.iter().map(|plate| plate.zone).collect();
    if ap_outcome(&unarmed, &zones).0 != RayOutcome::Overpenetration {
        return None;
    }

    Some(ArmingPlate { material: plate.material, thickness: TenthsMm::from_thickness(plate.thickness) })
}

/// The citadel seen by horizontal rays from either side. `None` without
/// citadel plates.
pub(crate) fn side_geometry(meshes: &[InteractiveArmorMesh]) -> Result<Option<CitadelGeometry>, QueryError> {
    if !has_citadel_plates(meshes) {
        return Ok(None);
    }

    let starboard = cast_grid(meshes, line_of_fire(Degrees::new(0.0), Degrees::new(0.0)), SIDE_SPACING)?;
    let port = cast_grid(meshes, line_of_fire(Degrees::new(180.0), Degrees::new(0.0)), SIDE_SPACING)?;

    Ok(citadel_geometry(meshes, &starboard.rays, &port.rays, SIDE_SPACING, bow_position(meshes)))
}

/// The starboard side's share of the citadel face above the waterline.
fn side_above_waterline(geometry: &CitadelGeometry) -> Option<Percent> {
    geometry.exposure.sides.first().and_then(|side| side.above_waterline_pct)
}

/// Rates over `tallies`, the mean of each sampled range's shares. `None`
/// when no ray struck armor.
fn summarize(tallies: &[Tally], ammo: Ammo) -> Option<Summary> {
    let struck: Vec<&Tally> = tallies.iter().filter(|tally| tally.rays > 0).collect();
    if struck.is_empty() {
        return None;
    }

    let mean = |field: fn(&Tally) -> usize| -> f32 {
        let total: f32 = struck.iter().map(|tally| field(tally) as f32 / tally.rays as f32).sum();
        total * 100.0 / struck.len() as f32
    };
    let ap = ammo == Ammo::Ap;
    let citadel = mean(|tally| tally.citadel);
    let penetration = mean(|tally| tally.penetration);

    Some(Summary {
        rays: struck.iter().map(|tally| tally.rays).sum(),
        rates: OutcomeRates {
            citadel_pct: percent(citadel),
            penetration_pct: percent(penetration),
            overpenetration_pct: ap.then(|| percent(mean(|tally| tally.overpenetration))),
            ricochet_pct: percent(mean(|tally| tally.ricochet)),
            shatter_pct: percent(mean(|tally| tally.shatter)),
            fuse_armed_pct: ap.then(|| percent(mean(|tally| tally.fuse_armed))),
        },
        damaging: percent(citadel + penetration),
    })
}

fn band_reports(
    set: &ShooterSet,
    traces: &[ShooterTraces],
    has_citadel: bool,
    peers: &[&PeerMetrics],
) -> Vec<BandReport> {
    RangeBand::ALL
        .into_iter()
        .map(|band| {
            let mut groups = Vec::new();
            let mut excluded = Vec::new();
            for (index, shooter) in set.shooters.iter().enumerate() {
                let ranges = shooter.ranges(band);
                if ranges.is_empty() {
                    excluded.push(Excluded {
                        group: shooter.report.group.clone(),
                        max_range_km: shooter.report.max_range_km,
                    });
                    continue;
                }

                let aspects = Aspect::ALL
                    .into_iter()
                    .filter_map(|aspect| {
                        let condition = Condition { band, aspect };
                        let summary = summarize(traces[index].tallies.get(&condition)?, shooter.report.ammo)?;
                        let ranks = (!peers.is_empty())
                            .then(|| ranks(ShooterIndex(index), condition, &summary, has_citadel, peers));
                        Some(AspectResult { aspect, rays: summary.rays, rates: summary.rates, ranks })
                    })
                    .collect();
                groups.push(GroupResult {
                    shooter: ShooterIndex(index),
                    group: shooter.report.group.clone(),
                    ranges_km: ranges,
                    aspects,
                });
            }

            BandReport { band, label: band.label(), groups, excluded }
        })
        .collect()
}

fn ranks(
    shooter: ShooterIndex,
    condition: Condition,
    summary: &Summary,
    has_citadel: bool,
    peers: &[&PeerMetrics],
) -> Ranks {
    let values: Vec<&PeerCondition> = peers
        .iter()
        .filter_map(|peer| {
            peer.conditions.iter().find(|entry| entry.shooter == shooter && entry.condition == condition)
        })
        .collect();

    Ranks {
        citadel: has_citadel
            .then(|| rank(summary.rates.citadel_pct, values.iter().map(|entry| entry.citadel)))
            .flatten(),
        damaging: rank(summary.damaging, values.iter().map(|entry| entry.damaging)),
        fuse_armed: summary
            .rates
            .fuse_armed_pct
            .and_then(|value| rank(value, values.iter().filter_map(|entry| entry.fuse_armed))),
    }
}

fn rank(value: Percent, peers: impl Iterator<Item = Percent>) -> Option<Rank> {
    rank_values(value.value(), peers.map(Percent::value))
}

/// `None` without peer values.
fn rank_values(value: f32, peers: impl Iterator<Item = f32>) -> Option<Rank> {
    let mut higher = 0;
    let mut lower = 0;
    let mut total = 0;
    for peer in peers {
        total += 1;
        if peer > value {
            higher += 1;
        } else if peer < value {
            lower += 1;
        }
    }
    if total == 0 {
        return None;
    }

    let share = |count: usize| Percent::new((count as f32 * 100.0 / total as f32).round());
    Some(Rank { higher_pct: share(higher), lower_pct: share(lower), compared: total })
}

fn citadel_position(geometry: &CitadelGeometry, peers: &[&PeerMetrics]) -> CitadelPosition {
    let citadels: Vec<&PeerCitadel> = peers.iter().filter_map(|peer| peer.citadel.as_ref()).collect();
    let roof = geometry.exposure.roof_above_waterline_m;
    let side = side_above_waterline(geometry);

    CitadelPosition {
        roof_above_waterline_m: roof,
        floor_m: geometry.exposure.floor_m,
        side_above_waterline_pct: side,
        roof_rank: rank_values(roof.value(), citadels.iter().map(|citadel| citadel.roof_above_waterline.value())),
        side_above_waterline_rank: side
            .and_then(|side| rank(side, citadels.iter().filter_map(|citadel| citadel.side_above_waterline))),
    }
}

fn fuse_arming(set: &ShooterSet, traces: &[ShooterTraces], name: impl Fn(ArmorMaterial) -> String) -> Vec<FuseArming> {
    const PLATE_LIMIT: usize = 3;

    set.shooters
        .iter()
        .zip(traces)
        .enumerate()
        .filter_map(|(index, (shooter, traces))| {
            let threshold = shooter.report.fuse_threshold_mm?;
            let rays: usize = traces.arming.values().sum();
            if rays == 0 {
                return None;
            }
            let total: usize = traces.tallies.values().flatten().map(|tally| tally.rays).sum();

            let mut plates: Vec<(&ArmingPlate, &usize)> = traces.arming.iter().collect();
            plates.sort_by(|left, right| right.1.cmp(left.1).then_with(|| left.0.cmp(right.0)));
            let plates = plates
                .into_iter()
                .take(PLATE_LIMIT)
                .map(|(plate, rays)| ArmingPlateReport {
                    plate: name(plate.material),
                    thickness_mm: plate.thickness.to_mm(),
                    rays: *rays,
                })
                .collect();

            Some(FuseArming {
                shooter: ShooterIndex(index),
                group: shooter.report.group.clone(),
                fuse_threshold_mm: Millimeters::new(round1(threshold.value())),
                rays,
                share_pct: percent(rays as f32 * 100.0 / total as f32),
                plates,
            })
        })
        .collect()
}

fn comparison(
    key: ComparisonKey,
    ship: &Ship,
    peers: &[&PeerMetrics],
    copies: usize,
    without_armor_model: usize,
) -> Comparison {
    const EXAMPLE_LIMIT: usize = 4;

    // The table lists tech tree ships first; peers of the ship's own tier
    // lead.
    let mut examples: Vec<&PeerMetrics> = peers.to_vec();
    examples.sort_by_key(|peer| peer.tier != ship.tier);
    let citadels: Vec<&PeerCitadel> = peers.iter().filter_map(|peer| peer.citadel.as_ref()).collect();

    Comparison {
        class: key.class,
        tiers: TierRange::adjacent(key.tier),
        peers: peers.len(),
        examples: examples.into_iter().take(EXAMPLE_LIMIT).map(|peer| peer.name.clone()).collect(),
        copies,
        without_armor_model,
        with_citadel: citadels.len(),
        with_turtleback: (!citadels.is_empty()).then(|| citadels.iter().filter(|citadel| citadel.turtleback).count()),
    }
}

fn verdict(
    ship: &Ship,
    set: &ShooterSet,
    bands: &[BandReport],
    properties: &ArmorProperties,
    comparison: &Comparison,
) -> String {
    let peers =
        format!("tier {}-{} {}", comparison.tiers.min.0, comparison.tiers.max.0, class_plural(comparison.class));
    let compared = |rank: Option<Rank>| match rank {
        Some(rank) => format!(" ({})", relative(rank, &peers)),
        None => String::new(),
    };
    let shooters = || set.shooters.iter().enumerate().map(|(index, shooter)| (ShooterIndex(index), shooter));
    let mut sentences = Vec::new();
    if let Some(note) = citadel_metrics(ship.class) {
        sentences.push(format!("Citadel metrics {note}"));
    }

    if properties.citadel.is_some() && citadel_metrics(ship.class).is_none() {
        // A battleship meets battleship AP of both calibers; other ships the
        // most common battleship caliber and heavy cruiser AP.
        let first_battleship = shooters().find(|(_, shooter)| shooter.report.role == ShooterRole::BattleshipAp);
        let threats = shooters().filter(|(index, shooter)| match shooter.report.role {
            ShooterRole::BattleshipAp => {
                ship.class == ShipClass::Battleship || first_battleship.is_some_and(|(first, _)| first == *index)
            }
            ShooterRole::HeavyCruiserAp => ship.class != ShipClass::Battleship,
            _ => false,
        });
        for (index, shooter) in threats {
            let Some(group) = group_result(bands, index, RangeBand::Moderate)
                .or_else(|| group_result(bands, index, RangeBand::Close))
            else {
                continue;
            };
            let (Some(broadside), Some(angled)) =
                (aspect_result(group, Aspect::Broadside), aspect_result(group, Aspect::Angled30))
            else {
                continue;
            };
            let citadel_rank = |result: &AspectResult| result.ranks.and_then(|ranks| ranks.citadel);
            sentences.push(format!(
                "{} ({}) at {}: citadels on {}% of {} hits{} and {}% {}{}.",
                capitalize_first(&shooter.report.group),
                shooter.name,
                span(&group.ranges_km),
                broadside.rates.citadel_pct.value(),
                Aspect::Broadside.label(),
                compared(citadel_rank(broadside)),
                angled.rates.citadel_pct.value(),
                Aspect::Angled30.label(),
                compared(citadel_rank(angled)),
            ));
        }
    } else {
        let threats = shooters().filter(|(_, shooter)| {
            matches!(shooter.report.role, ShooterRole::DestroyerAp | ShooterRole::LightCruiserAp)
        });
        for (index, shooter) in threats {
            let Some(group) = group_result(bands, index, RangeBand::Close) else {
                continue;
            };
            let Some(broadside) = aspect_result(group, Aspect::Broadside) else {
                continue;
            };
            let damaging = broadside.rates.citadel_pct.value() + broadside.rates.penetration_pct.value();
            sentences.push(format!(
                "{} ({}) at {}: {}% of broadside hits detonate inside the ship{}; {}% overpenetrate.",
                capitalize_first(&shooter.report.group),
                shooter.name,
                span(&group.ranges_km),
                round1(damaging),
                compared(broadside.ranks.and_then(|ranks| ranks.damaging)),
                broadside.rates.overpenetration_pct.map_or(0.0, Percent::value),
            ));
        }
    }

    for arming in &properties.fuse_arming {
        let shooter = &set.shooters[arming.shooter.0];
        if shooter.report.role != ShooterRole::DestroyerAp {
            continue;
        }
        let Some(group) = group_result(bands, arming.shooter, RangeBand::Close) else {
            continue;
        };
        let Some(broadside) = aspect_result(group, Aspect::Broadside) else {
            continue;
        };
        let Some(share) = broadside.rates.fuse_armed_pct.filter(|share| share.value() > 0.0) else {
            continue;
        };
        let plates: Vec<String> = arming
            .plates
            .iter()
            .take(2)
            .map(|plate| format!("{} {} mm", plate.plate, plate.thickness_mm.value()))
            .collect();
        sentences.push(format!(
            "Fuse arming by {} ({}; a plate of {} mm or more after normalization arms it) at {}: on {}% of broadside hits a plate arms the fuse of a shell that would otherwise overpenetrate, and it detonates inside{}. Over all bands and aspects the arming plates are mostly {}.",
            arming.group,
            shooter.name,
            arming.fuse_threshold_mm.value(),
            span(&group.ranges_km),
            share.value(),
            compared(broadside.ranks.and_then(|ranks| ranks.fuse_armed)),
            plates.join(" and "),
        ));
    }

    if let Some(turtleback) = &properties.turtleback {
        let others = match comparison.with_turtleback {
            Some(count) => format!(" ({count} of the {} {peers} with a citadel have one)", comparison.with_citadel),
            None => String::new(),
        };
        sentences.push(format!(
            "Turtleback: a {} mm deck sloped {} deg from vertical behind a {} mm belt{others}.",
            turtleback.median_deck_mm.value(),
            turtleback.slope_deg.value(),
            turtleback.median_belt_mm.value(),
        ));
    }

    match &properties.citadel {
        _ if citadel_metrics(ship.class).is_some() => {}
        Some(citadel) => {
            let roof = citadel.roof_above_waterline_m.value();
            let position = if roof >= 0.0 {
                format!("{roof} m above the waterline")
            } else {
                format!("{} m below the waterline", -roof)
            };
            let side = match citadel.side_above_waterline_pct {
                Some(share) => format!("; {}% of its side face is above the waterline", share.value()),
                None => String::new(),
            };
            sentences.push(format!("Citadel roof {position}{}{side}.", compared(citadel.roof_rank)));
        }
        None => sentences.push(format!("{} has no citadel plates.", ship.name)),
    }

    sentences.join(" ")
}

/// For a class without a citadel, why citadel metrics do not apply to it.
fn citadel_metrics(class: ShipClass) -> Option<&'static str> {
    match class {
        ShipClass::Destroyer => Some(
            "do not apply: destroyers have no citadel, so citadel rates are zero for every destroyer and a missing citadel is no weakness. Judge destroyer protection by health and by how often hits penetrate or overpenetrate.",
        ),
        ShipClass::Submarine => Some(
            "do not apply: submarines have no citadel, so citadel rates are zero for every submarine and a missing citadel is no weakness.",
        ),
        ShipClass::Cruiser | ShipClass::Battleship | ShipClass::AircraftCarrier => None,
    }
}

fn group_result(bands: &[BandReport], shooter: ShooterIndex, band: RangeBand) -> Option<&GroupResult> {
    let report = bands.iter().find(|report| report.band == band)?;

    report.groups.iter().find(|group| group.shooter == shooter)
}

fn aspect_result(group: &GroupResult, aspect: Aspect) -> Option<&AspectResult> {
    group.aspects.iter().find(|result| result.aspect == aspect)
}

/// "lower than 80% of 52 tier 9-11 cruisers", from the side most of the
/// compared peers are on.
fn relative(rank: Rank, peers: &str) -> String {
    let count = rank.compared;
    let higher = rank.higher_pct.value();
    let lower = rank.lower_pct.value();
    let equal = (100.0 - higher - lower).max(0.0);
    if equal >= higher.max(lower) {
        format!("the same as {equal}% of {count} {peers}")
    } else if higher >= 100.0 {
        format!("lower than all {count} {peers}")
    } else if lower >= 100.0 {
        format!("higher than all {count} {peers}")
    } else if higher >= lower {
        format!("lower than {higher}% of {count} {peers}")
    } else {
        format!("higher than {lower}% of {count} {peers}")
    }
}

/// "12-14 km", or "10 km" for one range.
fn span(ranges: &[Km]) -> String {
    match (ranges.first(), ranges.last()) {
        (Some(first), Some(last)) if first.value() != last.value() => format!("{}-{} km", first.value(), last.value()),
        (Some(first), _) => format!("{} km", first.value()),
        _ => String::new(),
    }
}

pub(crate) fn class_plural(class: ShipClass) -> &'static str {
    match class {
        ShipClass::Destroyer => "destroyers",
        ShipClass::Cruiser => "cruisers",
        ShipClass::Battleship => "battleships",
        ShipClass::AircraftCarrier => "aircraft carriers",
        ShipClass::Submarine => "submarines",
    }
}

fn capitalize_first(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

fn percent(value: f32) -> Percent {
    Percent::new(round1(value))
}

fn round1(value: f32) -> f32 {
    // Adding zero turns a negative zero into zero.
    (value * 10.0).round() / 10.0 + 0.0
}

/// The rates of `metrics` against each shooter group at each band and
/// aspect, with the same rates of `peers`.
fn matchups_of(metrics: &PeerMetrics, peers: &[&PeerMetrics], set: &ShooterSet) -> Vec<Matchup> {
    let with_citadel: Vec<&PeerMetrics> = peers.iter().copied().filter(|peer| peer.citadel.is_some()).collect();

    metrics
        .conditions
        .iter()
        .map(|entry| {
            let shooter = &set.shooters[entry.shooter.0];
            let measured = |value: Percent, peers: &[&PeerMetrics], field: fn(&PeerCondition) -> Option<Percent>| {
                let peers = peers
                    .iter()
                    .filter_map(|peer| {
                        let same = peer
                            .conditions
                            .iter()
                            .find(|other| other.shooter == entry.shooter && other.condition == entry.condition)?;
                        field(same)
                    })
                    .map(Percent::value)
                    .collect();
                Measured { value, peers }
            };
            let citadel = (shooter.report.ammo == Ammo::Ap && metrics.citadel.is_some())
                .then(|| measured(entry.citadel, &with_citadel, |same| Some(same.citadel)));

            Matchup {
                shooter: shooter.report.group.clone(),
                shooter_ship: shooter.name.clone(),
                role: shooter.report.role,
                band: entry.condition.band,
                ranges: shooter.ranges(entry.condition.band),
                aspect: entry.condition.aspect,
                citadel,
                damaging: measured(entry.damaging, peers, |same| Some(same.damaging)),
                overpenetration: entry.overpenetration.map(|value| measured(value, peers, |same| same.overpenetration)),
            }
        })
        .collect()
}

fn is_zero(value: &usize) -> bool {
    *value == 0
}
