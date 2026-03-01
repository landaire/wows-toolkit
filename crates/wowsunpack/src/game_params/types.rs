use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::HashSet;
use std::ops::Add;
use std::ops::Mul;
use std::ops::Sub;

use bon::Builder;
use variantly::Variantly;

use crate::Rc;
use crate::data::ResourceLoader;
use crate::game_types::GameParamId;

use super::provider::GameMetadataProvider;

/// Conversion factor: 1 BigWorld unit = 30 meters.
const BW_TO_METERS: f32 = 30.0;

/// Conversion factor: 1 BigWorld unit = 15 ship-model units.
/// Ship geometry (armor meshes, hull models) uses this coordinate space
/// where 1 ship-model unit = 2 real meters (= 30 / 15).
const BW_TO_SHIP: f32 = 15.0;

/// Per-material armor thickness map.
///
/// Outer key = collision material ID (0–254).
/// Inner key = model_index (1-based armor model ordinal).
/// Value = thickness in mm for that model_index.
///
/// The game registers the same geometry multiple times with different
/// `model_index` values; each registration represents a separate armor layer.
pub type ArmorMap = HashMap<u32, BTreeMap<u32, f32>>;

/// Distance in meters.
#[derive(Clone, Copy, Debug, Default, PartialEq, PartialOrd)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct Meters(f32);

/// Distance in BigWorld coordinate units (1 BW unit = 30 meters).
#[derive(Clone, Copy, Debug, Default, PartialEq, PartialOrd)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct BigWorldDistance(f32);

/// Distance in ship-model coordinate units (1 unit = 2 meters).
/// Ship geometry (armor meshes, hull visual models) uses this coordinate space.
/// The game defines BW_TO_SHIP = 15, meaning 1 BigWorld unit = 15 ship-model units,
/// so 1 ship-model unit = BW_TO_METERS / BW_TO_SHIP = 30 / 15 = 2 meters.
#[derive(Clone, Copy, Debug, Default, PartialEq, PartialOrd)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct ShipModelDistance(f32);

/// Distance in kilometers.
#[derive(Clone, Copy, Debug, Default, PartialEq, PartialOrd)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct Km(f32);

/// Distance in millimeters.
#[derive(Clone, Copy, Debug, Default, PartialEq, PartialOrd)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct Millimeters(f32);

// --- Construction ---

impl From<f32> for Meters {
    fn from(v: f32) -> Self {
        Self(v)
    }
}
impl From<i32> for Meters {
    fn from(v: i32) -> Self {
        Self(v as f32)
    }
}

impl From<f32> for BigWorldDistance {
    fn from(v: f32) -> Self {
        Self(v)
    }
}
impl From<i32> for BigWorldDistance {
    fn from(v: i32) -> Self {
        Self(v as f32)
    }
}

impl From<f32> for Km {
    fn from(v: f32) -> Self {
        Self(v)
    }
}
impl From<i32> for Km {
    fn from(v: i32) -> Self {
        Self(v as f32)
    }
}

impl From<f32> for Millimeters {
    fn from(v: f32) -> Self {
        Self(v)
    }
}
impl From<i32> for Millimeters {
    fn from(v: i32) -> Self {
        Self(v as f32)
    }
}

// --- Read access and unit conversions ---

impl Meters {
    /// Const constructor for use in static/const contexts.
    pub const fn new(v: f32) -> Self {
        Self(v)
    }

    pub fn value(self) -> f32 {
        self.0
    }
    pub fn to_bigworld(self) -> BigWorldDistance {
        BigWorldDistance(self.0 / BW_TO_METERS)
    }
    /// Convert to ship-model units (1 unit = 2 meters).
    /// Use this for distances that will be compared against ship geometry
    /// (armor meshes, hull models), which are in ship-model coordinates.
    pub fn to_ship_model(self) -> ShipModelDistance {
        ShipModelDistance(self.0 * BW_TO_SHIP / BW_TO_METERS)
    }
    pub fn to_km(self) -> Km {
        Km(self.0 / 1000.0)
    }
    pub fn to_mm(self) -> Millimeters {
        Millimeters(self.0 * 1000.0)
    }
}

impl BigWorldDistance {
    pub fn value(self) -> f32 {
        self.0
    }
    pub fn to_meters(self) -> Meters {
        Meters(self.0 * BW_TO_METERS)
    }
}

impl ShipModelDistance {
    pub fn value(self) -> f32 {
        self.0
    }
    pub fn to_meters(self) -> Meters {
        Meters(self.0 * BW_TO_METERS / BW_TO_SHIP)
    }
    pub fn to_bigworld(self) -> BigWorldDistance {
        BigWorldDistance(self.0 / BW_TO_SHIP)
    }
}

impl Km {
    /// Const constructor for use in static/const contexts.
    pub const fn new(v: f32) -> Self {
        Self(v)
    }

    pub fn value(self) -> f32 {
        self.0
    }
    pub fn to_meters(self) -> Meters {
        Meters(self.0 * 1000.0)
    }
}

impl Millimeters {
    /// Const constructor for use in static/const contexts.
    pub const fn new(v: f32) -> Self {
        Self(v)
    }

    pub fn value(self) -> f32 {
        self.0
    }
    pub fn to_meters(self) -> Meters {
        Meters(self.0 / 1000.0)
    }
    pub fn to_bigworld(self) -> BigWorldDistance {
        self.to_meters().to_bigworld()
    }
}

// --- Scalar multiplication (dimensionless coefficients) ---

impl Mul<f32> for Meters {
    type Output = Meters;
    fn mul(self, rhs: f32) -> Meters {
        Meters(self.0 * rhs)
    }
}

impl Mul<f32> for BigWorldDistance {
    type Output = BigWorldDistance;
    fn mul(self, rhs: f32) -> BigWorldDistance {
        BigWorldDistance(self.0 * rhs)
    }
}

impl Mul<f32> for Km {
    type Output = Km;
    fn mul(self, rhs: f32) -> Km {
        Km(self.0 * rhs)
    }
}

impl Mul<f32> for Millimeters {
    type Output = Millimeters;
    fn mul(self, rhs: f32) -> Millimeters {
        Millimeters(self.0 * rhs)
    }
}

// --- Same-type arithmetic ---

impl Add for Meters {
    type Output = Meters;
    fn add(self, rhs: Meters) -> Meters {
        Meters(self.0 + rhs.0)
    }
}
impl Sub for Meters {
    type Output = Meters;
    fn sub(self, rhs: Meters) -> Meters {
        Meters(self.0 - rhs.0)
    }
}

impl Add for BigWorldDistance {
    type Output = BigWorldDistance;
    fn add(self, rhs: BigWorldDistance) -> BigWorldDistance {
        BigWorldDistance(self.0 + rhs.0)
    }
}
impl Sub for BigWorldDistance {
    type Output = BigWorldDistance;
    fn sub(self, rhs: BigWorldDistance) -> BigWorldDistance {
        BigWorldDistance(self.0 - rhs.0)
    }
}

impl Add for Km {
    type Output = Km;
    fn add(self, rhs: Km) -> Km {
        Km(self.0 + rhs.0)
    }
}
impl Sub for Km {
    type Output = Km;
    fn sub(self, rhs: Km) -> Km {
        Km(self.0 - rhs.0)
    }
}

impl Add for Millimeters {
    type Output = Millimeters;
    fn add(self, rhs: Millimeters) -> Millimeters {
        Millimeters(self.0 + rhs.0)
    }
}
impl Sub for Millimeters {
    type Output = Millimeters;
    fn sub(self, rhs: Millimeters) -> Millimeters {
        Millimeters(self.0 - rhs.0)
    }
}

// --- Cross-type arithmetic (converts RHS to LHS unit, returns LHS type) ---

impl Add<BigWorldDistance> for Meters {
    type Output = Meters;
    fn add(self, rhs: BigWorldDistance) -> Meters {
        Meters(self.0 + rhs.to_meters().0)
    }
}
impl Sub<BigWorldDistance> for Meters {
    type Output = Meters;
    fn sub(self, rhs: BigWorldDistance) -> Meters {
        Meters(self.0 - rhs.to_meters().0)
    }
}

impl Add<Meters> for BigWorldDistance {
    type Output = BigWorldDistance;
    fn add(self, rhs: Meters) -> BigWorldDistance {
        BigWorldDistance(self.0 + rhs.to_bigworld().0)
    }
}
impl Sub<Meters> for BigWorldDistance {
    type Output = BigWorldDistance;
    fn sub(self, rhs: Meters) -> BigWorldDistance {
        BigWorldDistance(self.0 - rhs.to_bigworld().0)
    }
}

impl Add<Km> for Meters {
    type Output = Meters;
    fn add(self, rhs: Km) -> Meters {
        Meters(self.0 + rhs.to_meters().0)
    }
}
impl Sub<Km> for Meters {
    type Output = Meters;
    fn sub(self, rhs: Km) -> Meters {
        Meters(self.0 - rhs.to_meters().0)
    }
}

impl Add<Meters> for Km {
    type Output = Km;
    fn add(self, rhs: Meters) -> Km {
        Km(self.0 + rhs.to_km().0)
    }
}
impl Sub<Meters> for Km {
    type Output = Km;
    fn sub(self, rhs: Meters) -> Km {
        Km(self.0 - rhs.to_km().0)
    }
}

// --- Scalar division (for averaging, etc.) ---

impl std::ops::Div<f32> for Meters {
    type Output = Meters;
    fn div(self, rhs: f32) -> Meters {
        Meters(self.0 / rhs)
    }
}

impl std::ops::Div<f32> for BigWorldDistance {
    type Output = BigWorldDistance;
    fn div(self, rhs: f32) -> BigWorldDistance {
        BigWorldDistance(self.0 / rhs)
    }
}

impl std::ops::Div<f32> for Km {
    type Output = Km;
    fn div(self, rhs: f32) -> Km {
        Km(self.0 / rhs)
    }
}

impl std::ops::Div<f32> for Millimeters {
    type Output = Millimeters;
    fn div(self, rhs: f32) -> Millimeters {
        Millimeters(self.0 / rhs)
    }
}

// --- Sum (for iterator aggregation) ---

impl std::iter::Sum for Meters {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        Meters(iter.map(|m| m.0).sum())
    }
}

impl std::iter::Sum for BigWorldDistance {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        BigWorldDistance(iter.map(|d| d.0).sum())
    }
}

impl std::iter::Sum for Km {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        Km(iter.map(|k| k.0).sum())
    }
}

impl std::iter::Sum for Millimeters {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        Millimeters(iter.map(|m| m.0).sum())
    }
}

// --- Cross-type comparison (converts to common unit for PartialEq/PartialOrd) ---

impl PartialEq<BigWorldDistance> for Meters {
    fn eq(&self, other: &BigWorldDistance) -> bool {
        self.0 == other.to_meters().0
    }
}
impl PartialOrd<BigWorldDistance> for Meters {
    fn partial_cmp(&self, other: &BigWorldDistance) -> Option<std::cmp::Ordering> {
        self.0.partial_cmp(&other.to_meters().0)
    }
}

impl PartialEq<Meters> for BigWorldDistance {
    fn eq(&self, other: &Meters) -> bool {
        self.0 == other.to_bigworld().0
    }
}
impl PartialOrd<Meters> for BigWorldDistance {
    fn partial_cmp(&self, other: &Meters) -> Option<std::cmp::Ordering> {
        self.0.partial_cmp(&other.to_bigworld().0)
    }
}

#[derive(Clone, Copy, Debug, Variantly, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub enum Species {
    AAircraft,
    AbilitiesUnit,
    AirBase,
    AirCarrier,
    Airship,
    AntiAircraft,
    Artillery,
    ArtilleryUnit,
    Auxiliary,
    Battleship,
    Bomb,
    Bomber,
    BuildingType,
    Camoboost,
    Camouflage,
    Campaign,
    CoastalArtillery,
    CollectionAlbum,
    CollectionCard,
    Complex,
    Cruiser,
    DCharge,
    DeathSettings,
    DepthCharge,
    Destroyer,
    Dive,
    DiveBomberTypeUnit,
    DogTagDoll,
    DogTagItem,
    DogTagSlotsScheme,
    DogTagUnique,
    Drop,
    DropVisual,
    EngineUnit,
    Ensign,
    Event,
    Fake,
    Fighter,
    FighterTypeUnit,
    FireControl,
    Flags,
    FlightControlUnit,
    Generator,
    GlobalWeather,
    Globalboost,
    Hull,
    HullUnit,
    IndividualTask,
    Laser,
    LocalWeather,
    MSkin,
    Main,
    MapBorder,
    Military,
    Mine,
    Mission,
    Modifier,
    Multiboost,
    NewbieQuest,
    Operation,
    Permoflage,
    PlaneTracer,
    PrimaryWeaponsUnit,
    RayTower,
    Rocket,
    Scout,
    Search,
    Secondary,
    SecondaryWeaponsUnit,
    SensorTower,
    Sinking,
    Skin,
    Skip,
    SkipBomb,
    SkipBomberTypeUnit,
    SonarUnit,
    SpaceStation,
    Submarine,
    SuoUnit,
    Task,
    Torpedo,
    TorpedoBomberTypeUnit,
    TorpedoesUnit,
    Upgrade,
    Wave,
    Null,
}

impl Species {
    pub fn from_name(name: &str) -> crate::recognized::Recognized<Self> {
        use crate::recognized::Recognized;
        match name {
            "AAircraft" => Recognized::Known(Self::AAircraft),
            "AbilitiesUnit" => Recognized::Known(Self::AbilitiesUnit),
            "AirBase" => Recognized::Known(Self::AirBase),
            "AirCarrier" => Recognized::Known(Self::AirCarrier),
            "Airship" => Recognized::Known(Self::Airship),
            "AntiAircraft" => Recognized::Known(Self::AntiAircraft),
            "Artillery" => Recognized::Known(Self::Artillery),
            "ArtilleryUnit" => Recognized::Known(Self::ArtilleryUnit),
            "Auxiliary" => Recognized::Known(Self::Auxiliary),
            "Battleship" => Recognized::Known(Self::Battleship),
            "Bomb" => Recognized::Known(Self::Bomb),
            "Bomber" => Recognized::Known(Self::Bomber),
            "BuildingType" => Recognized::Known(Self::BuildingType),
            "Camoboost" => Recognized::Known(Self::Camoboost),
            "Camouflage" => Recognized::Known(Self::Camouflage),
            "Campaign" => Recognized::Known(Self::Campaign),
            "CoastalArtillery" => Recognized::Known(Self::CoastalArtillery),
            "CollectionAlbum" => Recognized::Known(Self::CollectionAlbum),
            "CollectionCard" => Recognized::Known(Self::CollectionCard),
            "Complex" => Recognized::Known(Self::Complex),
            "Cruiser" => Recognized::Known(Self::Cruiser),
            "DCharge" => Recognized::Known(Self::DCharge),
            "DeathSettings" => Recognized::Known(Self::DeathSettings),
            "DepthCharge" => Recognized::Known(Self::DepthCharge),
            "Destroyer" => Recognized::Known(Self::Destroyer),
            "Dive" => Recognized::Known(Self::Dive),
            "DiveBomberTypeUnit" => Recognized::Known(Self::DiveBomberTypeUnit),
            "DogTagDoll" => Recognized::Known(Self::DogTagDoll),
            "DogTagItem" => Recognized::Known(Self::DogTagItem),
            "DogTagSlotsScheme" => Recognized::Known(Self::DogTagSlotsScheme),
            "DogTagUnique" => Recognized::Known(Self::DogTagUnique),
            "Drop" => Recognized::Known(Self::Drop),
            "DropVisual" => Recognized::Known(Self::DropVisual),
            "EngineUnit" => Recognized::Known(Self::EngineUnit),
            "Ensign" => Recognized::Known(Self::Ensign),
            "Event" => Recognized::Known(Self::Event),
            "Fake" => Recognized::Known(Self::Fake),
            "Fighter" => Recognized::Known(Self::Fighter),
            "FighterTypeUnit" => Recognized::Known(Self::FighterTypeUnit),
            "Fire control" | "FireControl" => Recognized::Known(Self::FireControl),
            "Flags" => Recognized::Known(Self::Flags),
            "FlightControlUnit" => Recognized::Known(Self::FlightControlUnit),
            "Generator" => Recognized::Known(Self::Generator),
            "GlobalWeather" => Recognized::Known(Self::GlobalWeather),
            "Globalboost" => Recognized::Known(Self::Globalboost),
            "Hull" => Recognized::Known(Self::Hull),
            "HullUnit" => Recognized::Known(Self::HullUnit),
            "IndividualTask" => Recognized::Known(Self::IndividualTask),
            "Laser" => Recognized::Known(Self::Laser),
            "LocalWeather" => Recognized::Known(Self::LocalWeather),
            "MSkin" => Recognized::Known(Self::MSkin),
            "Main" => Recognized::Known(Self::Main),
            "MapBorder" => Recognized::Known(Self::MapBorder),
            "Military" => Recognized::Known(Self::Military),
            "Mine" => Recognized::Known(Self::Mine),
            "Mission" => Recognized::Known(Self::Mission),
            "Modifier" => Recognized::Known(Self::Modifier),
            "Multiboost" => Recognized::Known(Self::Multiboost),
            "NewbieQuest" => Recognized::Known(Self::NewbieQuest),
            "Operation" => Recognized::Known(Self::Operation),
            "Permoflage" => Recognized::Known(Self::Permoflage),
            "PlaneTracer" => Recognized::Known(Self::PlaneTracer),
            "PrimaryWeaponsUnit" => Recognized::Known(Self::PrimaryWeaponsUnit),
            "RayTower" => Recognized::Known(Self::RayTower),
            "Rocket" => Recognized::Known(Self::Rocket),
            "Scout" => Recognized::Known(Self::Scout),
            "Search" => Recognized::Known(Self::Search),
            "Secondary" => Recognized::Known(Self::Secondary),
            "SecondaryWeaponsUnit" => Recognized::Known(Self::SecondaryWeaponsUnit),
            "SensorTower" => Recognized::Known(Self::SensorTower),
            "Sinking" => Recognized::Known(Self::Sinking),
            "Skin" => Recognized::Known(Self::Skin),
            "Skip" => Recognized::Known(Self::Skip),
            "SkipBomb" => Recognized::Known(Self::SkipBomb),
            "SkipBomberTypeUnit" => Recognized::Known(Self::SkipBomberTypeUnit),
            "SonarUnit" => Recognized::Known(Self::SonarUnit),
            "SpaceStation" => Recognized::Known(Self::SpaceStation),
            "Submarine" => Recognized::Known(Self::Submarine),
            "SuoUnit" => Recognized::Known(Self::SuoUnit),
            "Task" => Recognized::Known(Self::Task),
            "Torpedo" => Recognized::Known(Self::Torpedo),
            "TorpedoBomberTypeUnit" => Recognized::Known(Self::TorpedoBomberTypeUnit),
            "TorpedoesUnit" => Recognized::Known(Self::TorpedoesUnit),
            "Upgrade" => Recognized::Known(Self::Upgrade),
            "Wave" => Recognized::Known(Self::Wave),
            "null" => Recognized::Known(Self::Null),
            other => Recognized::Unknown(other.to_string()),
        }
    }

    pub const fn name(&self) -> &'static str {
        match self {
            Self::AAircraft => "AAircraft",
            Self::AbilitiesUnit => "AbilitiesUnit",
            Self::AirBase => "AirBase",
            Self::AirCarrier => "AirCarrier",
            Self::Airship => "Airship",
            Self::AntiAircraft => "AntiAircraft",
            Self::Artillery => "Artillery",
            Self::ArtilleryUnit => "ArtilleryUnit",
            Self::Auxiliary => "Auxiliary",
            Self::Battleship => "Battleship",
            Self::Bomb => "Bomb",
            Self::Bomber => "Bomber",
            Self::BuildingType => "BuildingType",
            Self::Camoboost => "Camoboost",
            Self::Camouflage => "Camouflage",
            Self::Campaign => "Campaign",
            Self::CoastalArtillery => "CoastalArtillery",
            Self::CollectionAlbum => "CollectionAlbum",
            Self::CollectionCard => "CollectionCard",
            Self::Complex => "Complex",
            Self::Cruiser => "Cruiser",
            Self::DCharge => "DCharge",
            Self::DeathSettings => "DeathSettings",
            Self::DepthCharge => "DepthCharge",
            Self::Destroyer => "Destroyer",
            Self::Dive => "Dive",
            Self::DiveBomberTypeUnit => "DiveBomberTypeUnit",
            Self::DogTagDoll => "DogTagDoll",
            Self::DogTagItem => "DogTagItem",
            Self::DogTagSlotsScheme => "DogTagSlotsScheme",
            Self::DogTagUnique => "DogTagUnique",
            Self::Drop => "Drop",
            Self::DropVisual => "DropVisual",
            Self::EngineUnit => "EngineUnit",
            Self::Ensign => "Ensign",
            Self::Event => "Event",
            Self::Fake => "Fake",
            Self::Fighter => "Fighter",
            Self::FighterTypeUnit => "FighterTypeUnit",
            Self::FireControl => "Fire control",
            Self::Flags => "Flags",
            Self::FlightControlUnit => "FlightControlUnit",
            Self::Generator => "Generator",
            Self::GlobalWeather => "GlobalWeather",
            Self::Globalboost => "Globalboost",
            Self::Hull => "Hull",
            Self::HullUnit => "HullUnit",
            Self::IndividualTask => "IndividualTask",
            Self::Laser => "Laser",
            Self::LocalWeather => "LocalWeather",
            Self::MSkin => "MSkin",
            Self::Main => "Main",
            Self::MapBorder => "MapBorder",
            Self::Military => "Military",
            Self::Mine => "Mine",
            Self::Mission => "Mission",
            Self::Modifier => "Modifier",
            Self::Multiboost => "Multiboost",
            Self::NewbieQuest => "NewbieQuest",
            Self::Operation => "Operation",
            Self::Permoflage => "Permoflage",
            Self::PlaneTracer => "PlaneTracer",
            Self::PrimaryWeaponsUnit => "PrimaryWeaponsUnit",
            Self::RayTower => "RayTower",
            Self::Rocket => "Rocket",
            Self::Scout => "Scout",
            Self::Search => "Search",
            Self::Secondary => "Secondary",
            Self::SecondaryWeaponsUnit => "SecondaryWeaponsUnit",
            Self::SensorTower => "SensorTower",
            Self::Sinking => "Sinking",
            Self::Skin => "Skin",
            Self::Skip => "Skip",
            Self::SkipBomb => "SkipBomb",
            Self::SkipBomberTypeUnit => "SkipBomberTypeUnit",
            Self::SonarUnit => "SonarUnit",
            Self::SpaceStation => "SpaceStation",
            Self::Submarine => "Submarine",
            Self::SuoUnit => "SuoUnit",
            Self::Task => "Task",
            Self::Torpedo => "Torpedo",
            Self::TorpedoBomberTypeUnit => "TorpedoBomberTypeUnit",
            Self::TorpedoesUnit => "TorpedoesUnit",
            Self::Upgrade => "Upgrade",
            Self::Wave => "Wave",
            Self::Null => "null",
        }
    }

    pub fn translation_id(&self) -> String {
        format!("IDS_{}", self.name())
    }
}

#[derive(Builder, Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct Param {
    id: GameParamId,
    index: String,
    name: String,
    species: Option<crate::recognized::Recognized<Species>>,
    nation: String,
    data: ParamData,
}

impl Param {
    pub fn id(&self) -> GameParamId {
        self.id
    }

    pub fn index(&self) -> &str {
        self.index.as_ref()
    }

    pub fn name(&self) -> &str {
        self.name.as_ref()
    }

    pub fn species(&self) -> Option<&crate::recognized::Recognized<Species>> {
        self.species.as_ref()
    }

    pub fn nation(&self) -> &str {
        self.nation.as_ref()
    }

    pub fn data(&self) -> &ParamData {
        &self.data
    }

    /// Returns the Aircraft data if this param is an Aircraft type.
    pub fn aircraft(&self) -> Option<&Aircraft> {
        match &self.data {
            ParamData::Aircraft(a) => Some(a),
            _ => None,
        }
    }

    /// Returns the Vehicle data if this param is a Vehicle (ship) type.
    pub fn vehicle(&self) -> Option<&Vehicle> {
        match &self.data {
            ParamData::Vehicle(v) => Some(v),
            _ => None,
        }
    }

    /// Returns the Ability data if this param is an Ability (consumable) type.
    pub fn ability(&self) -> Option<&Ability> {
        match &self.data {
            ParamData::Ability(a) => Some(a),
            _ => None,
        }
    }

    /// Returns the Projectile data if this param is a Projectile type.
    pub fn projectile(&self) -> Option<&Projectile> {
        match &self.data {
            ParamData::Projectile(p) => Some(p),
            _ => None,
        }
    }

    /// Returns the Crew data if this param is a Crew type.
    pub fn crew(&self) -> Option<&Crew> {
        match &self.data {
            ParamData::Crew(c) => Some(c),
            _ => None,
        }
    }

    /// Returns the Modernization data if this param is a Modernization type.
    pub fn modernization(&self) -> Option<&Modernization> {
        match &self.data {
            ParamData::Modernization(m) => Some(m),
            _ => None,
        }
    }

    /// Returns the Drop data if this param is a Drop type.
    pub fn drop_data(&self) -> Option<&BuffDrop> {
        match &self.data {
            ParamData::Drop(d) => Some(d),
            _ => None,
        }
    }

    /// Returns the Exterior data if this param is an Exterior type.
    pub fn exterior(&self) -> Option<&Exterior> {
        match &self.data {
            ParamData::Exterior(e) => Some(e),
            _ => None,
        }
    }
}

#[derive(PartialEq, Eq, Hash, Debug, Clone, Copy, Variantly)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub enum ParamType {
    Ability,
    Achievement,
    AdjustmentShotActivator,
    Aircraft,
    BattleScript,
    Building,
    Campaign,
    Catapult,
    ClanSupply,
    Collection,
    Component,
    Crew,
    Director,
    DogTag,
    Drop,
    EventTrigger,
    Exterior,
    Finder,
    Gun,
    Modernization,
    Other,
    Projectile,
    Radar,
    RageModeProgressAction,
    Reward,
    RibbonActivator,
    Sfx,
    Ship,
    SwitchTrigger,
    SwitchVehicleVisualStateAction,
    TimerActivator,
    ToggleTriggerAction,
    Unit,
    VisibilityChangedActivator,
}

impl ParamType {
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "Ability" => Some(Self::Ability),
            "Achievement" => Some(Self::Achievement),
            "AdjustmentShotActivator" => Some(Self::AdjustmentShotActivator),
            "Aircraft" => Some(Self::Aircraft),
            "BattleScript" => Some(Self::BattleScript),
            "Building" => Some(Self::Building),
            "Campaign" => Some(Self::Campaign),
            "Catapult" => Some(Self::Catapult),
            "ClanSupply" => Some(Self::ClanSupply),
            "Collection" => Some(Self::Collection),
            "Component" => Some(Self::Component),
            "Crew" => Some(Self::Crew),
            "Director" => Some(Self::Director),
            "DogTag" => Some(Self::DogTag),
            "Drop" => Some(Self::Drop),
            "EventTrigger" => Some(Self::EventTrigger),
            "Exterior" => Some(Self::Exterior),
            "Finder" => Some(Self::Finder),
            "Gun" => Some(Self::Gun),
            "Modernization" => Some(Self::Modernization),
            "Other" => Some(Self::Other),
            "Projectile" => Some(Self::Projectile),
            "Radar" => Some(Self::Radar),
            "RageModeProgressAction" => Some(Self::RageModeProgressAction),
            "Reward" => Some(Self::Reward),
            "RibbonActivator" => Some(Self::RibbonActivator),
            "Sfx" => Some(Self::Sfx),
            "Ship" => Some(Self::Ship),
            "SwitchTrigger" => Some(Self::SwitchTrigger),
            "SwitchVehicleVisualStateAction" => Some(Self::SwitchVehicleVisualStateAction),
            "TimerActivator" => Some(Self::TimerActivator),
            "ToggleTriggerAction" => Some(Self::ToggleTriggerAction),
            "Unit" => Some(Self::Unit),
            "VisibilityChangedActivator" => Some(Self::VisibilityChangedActivator),
            _ => None,
        }
    }
}

// #[derive(Serialize, Deserialize, Clone, Builder, Debug)]
// pub struct VehicleAbility {
//     typ: String,

// }

/// Mount species from GameParams `typeinfo.species`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub enum MountSpecies {
    Main,
    Secondary,
    AAircraft,
    Torpedo,
    DCharge,
    FireControl,
    Search,
    MissileGun,
    Decoration,
}

impl MountSpecies {
    /// Parse from the GameParams `typeinfo.species` string value.
    pub fn from_gp_str(s: &str) -> Option<Self> {
        match s {
            "Main" => Some(Self::Main),
            "Secondary" => Some(Self::Secondary),
            "AAircraft" => Some(Self::AAircraft),
            "Torpedo" => Some(Self::Torpedo),
            "DCharge" => Some(Self::DCharge),
            "Fire control" => Some(Self::FireControl),
            "Search" => Some(Self::Search),
            "MissileGun" => Some(Self::MissileGun),
            "Decoration" => Some(Self::Decoration),
            _ => None,
        }
    }

    /// Display group name for Blender outliner hierarchy.
    pub fn display_group(&self) -> &'static str {
        match self {
            Self::Main => "Main Battery",
            Self::Secondary => "Secondary Battery",
            Self::AAircraft => "AA Guns",
            Self::Torpedo => "Torpedoes",
            Self::DCharge => "Depth Charges",
            Self::FireControl => "Fire Control",
            Self::Search => "Radar",
            Self::MissileGun => "Missiles",
            Self::Decoration => "Decorations",
        }
    }
}

/// A single mount point (hardpoint) within a ship component.
///
/// Each ship component (artillery, atba, etc.) has one or more mount points
/// identified by `HP_*` keys in the GameParams data. Each mount references a
/// 3D model file.
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct MountPoint {
    /// Hardpoint name, e.g. "HP_JGM_1".
    hp_name: String,
    /// Model path, e.g. "content/gameplay/.../JGM178.model".
    model_path: String,
    /// Per-mount armor thickness map from GameParams (e.g. `A_Artillery.HP_XXX.armor`).
    /// See [`ArmorMap`] for key/value semantics.
    #[cfg_attr(feature = "serde", serde(default, skip_serializing_if = "Option::is_none"))]
    mount_armor: Option<ArmorMap>,
    /// Mount species from GameParams `typeinfo.species`.
    #[cfg_attr(feature = "serde", serde(default, skip_serializing_if = "Option::is_none"))]
    species: Option<MountSpecies>,
    /// Pitch dead zones: regions where barrel elevation is clamped.
    /// Each entry is `[yaw_min_deg, yaw_max_deg, pitch_min_deg, pitch_max_deg]`.
    /// When the turret yaw falls in `[yaw_min, yaw_max]`, elevation is clamped
    /// to `[pitch_min, pitch_max]`.
    #[cfg_attr(feature = "serde", serde(default, skip_serializing_if = "Vec::is_empty"))]
    pitch_dead_zones: Vec<[f32; 4]>,
}

impl MountPoint {
    pub fn new(hp_name: String, model_path: String) -> Self {
        Self { hp_name, model_path, mount_armor: None, species: None, pitch_dead_zones: Vec::new() }
    }

    pub fn with_armor(
        hp_name: String,
        model_path: String,
        mount_armor: Option<ArmorMap>,
        species: Option<MountSpecies>,
        pitch_dead_zones: Vec<[f32; 4]>,
    ) -> Self {
        Self { hp_name, model_path, mount_armor, species, pitch_dead_zones }
    }

    pub fn hp_name(&self) -> &str {
        &self.hp_name
    }

    pub fn species(&self) -> Option<MountSpecies> {
        self.species
    }

    pub fn model_path(&self) -> &str {
        &self.model_path
    }

    pub fn mount_armor(&self) -> Option<&ArmorMap> {
        self.mount_armor.as_ref()
    }

    pub fn pitch_dead_zones(&self) -> &[[f32; 4]] {
        &self.pitch_dead_zones
    }

    /// Get the minimum barrel elevation (degrees) at a given yaw angle (degrees)
    /// by checking pitch dead zones. Returns 0.0 if no dead zone applies.
    pub fn min_pitch_at_yaw(&self, yaw_deg: f32) -> f32 {
        for dz in &self.pitch_dead_zones {
            let [yaw_min, yaw_max, pitch_min, _pitch_max] = *dz;
            if yaw_deg >= yaw_min && yaw_deg <= yaw_max {
                return pitch_min;
            }
        }
        0.0
    }
}

/// All mount points for a single component type within a hull upgrade.
#[derive(Clone, Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct ComponentMounts {
    /// The component name in GameParams (e.g. "AB1_Artillery").
    component_name: String,
    /// All HP_ mount points within this component.
    mounts: Vec<MountPoint>,
}

impl ComponentMounts {
    pub fn new(component_name: String, mounts: Vec<MountPoint>) -> Self {
        Self { component_name, mounts }
    }

    pub fn component_name(&self) -> &str {
        &self.component_name
    }

    pub fn mounts(&self) -> &[MountPoint] {
        &self.mounts
    }
}

/// Camouflage/exterior data from a GameParams `Exterior` entry.
#[derive(Clone, Builder, Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct Exterior {
    /// Camouflage texture scheme ID, e.g. "camo_permanent_1".
    #[cfg_attr(feature = "serde", serde(default))]
    camouflage: Option<String>,
    /// Translation key for display name, e.g. "IDS_PJES360_Yamato_Golden".
    #[cfg_attr(feature = "serde", serde(default))]
    title: Option<String>,
}

impl Exterior {
    pub fn camouflage(&self) -> Option<&str> {
        self.camouflage.as_deref()
    }

    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }
}

/// All range data associated with a specific hull upgrade.
///
/// Each hull upgrade in `ShipUpgradeInfo` (ucType = "_Hull") references specific
/// hull, artillery, and ATBA components. This struct captures the resolved range
/// data from those components.
#[derive(Clone, Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct HullUpgradeConfig {
    /// Sea detection range in km.
    pub detection_km: Km,
    /// Air detection range in km.
    pub air_detection_km: Km,
    /// Component type key → component name (e.g. "artillery" → "AB1_Artillery").
    #[cfg_attr(feature = "serde", serde(default))]
    pub component_names: HashMap<super::keys::ComponentType, String>,
    /// Model path from the hull component for this upgrade.
    #[cfg_attr(feature = "serde", serde(default))]
    pub hull_model_path: Option<String>,
    /// Ship draft (depth below waterline) in meters, from the hull component.
    #[cfg_attr(feature = "serde", serde(default))]
    pub draft: Option<Meters>,
    /// Normalized waterline offset from model origin.
    /// Range [-1, 1]: -1 = bottom of bounding box, 0 = pivot (model origin), +1 = top.
    /// Used to position the waterline plane on the ship model.
    #[cfg_attr(feature = "serde", serde(default))]
    pub dock_y_offset: Option<f32>,
    /// Mount points grouped by component type key (default selection).
    #[cfg_attr(feature = "serde", serde(default))]
    pub mounts_by_type: HashMap<super::keys::ComponentType, ComponentMounts>,
    /// All component name alternatives per type key
    /// (e.g. "artillery" -> ["AB1_Artillery", "AB2_Artillery"]).
    /// Only populated for types that have more than one option.
    #[cfg_attr(feature = "serde", serde(default))]
    pub component_alternatives: HashMap<super::keys::ComponentType, Vec<String>>,
    /// Mount points for non-default component alternatives, keyed by component name.
    /// The default component's mounts are in `mounts_by_type`; alternatives live here.
    #[cfg_attr(feature = "serde", serde(default))]
    pub alternative_mounts: HashMap<String, ComponentMounts>,
}

impl HullUpgradeConfig {
    /// Get the component name for a given component type (e.g. "artillery").
    pub fn component_name(&self, ct: super::keys::ComponentType) -> Option<&str> {
        self.component_names.get(&ct).map(|s| s.as_str())
    }

    /// Get the hull model path for this upgrade.
    pub fn hull_model_path(&self) -> Option<&str> {
        self.hull_model_path.as_deref()
    }

    /// Get the ship draft in meters.
    pub fn draft(&self) -> Option<Meters> {
        self.draft
    }

    /// Get the normalized waterline offset from model origin.
    /// Range [-1, 1]: -1 = bottom of bounding box, 0 = pivot, +1 = top.
    pub fn dock_y_offset(&self) -> Option<f32> {
        self.dock_y_offset
    }

    /// Get mount points for a specific component type.
    pub fn mounts(&self, ct: super::keys::ComponentType) -> Option<&[MountPoint]> {
        self.mounts_by_type.get(&ct).map(|cm| cm.mounts())
    }

    /// Iterate over all mount points across all component types (default selection).
    pub fn all_mount_points(&self) -> impl Iterator<Item = &MountPoint> {
        self.mounts_by_type.values().flat_map(|cm| cm.mounts.iter())
    }

    /// Get component alternatives for a given type key (empty if only one option).
    pub fn alternatives(&self, ct: super::keys::ComponentType) -> &[String] {
        self.component_alternatives.get(&ct).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// Iterate over all mount points, substituting overrides for specific component types.
    /// `overrides` maps component type key (e.g. "artillery") to the component name to use.
    pub fn mount_points_with_overrides<'a>(
        &'a self,
        overrides: &'a HashMap<super::keys::ComponentType, String>,
    ) -> impl Iterator<Item = &'a MountPoint> {
        self.mounts_by_type.iter().flat_map(move |(ct_key, default_cm)| {
            if let Some(override_name) = overrides.get(ct_key) {
                // Use alternative mounts if the override differs from default
                if override_name != &default_cm.component_name
                    && let Some(alt_cm) = self.alternative_mounts.get(override_name)
                {
                    return alt_cm.mounts.iter();
                }
            }
            default_cm.mounts.iter()
        })
    }
}

/// Ship configuration data extracted from GameParams.
///
/// Hull configs are keyed by the hull upgrade's GameParam ID so the renderer
/// can look up the player's equipped hull directly from `ShipConfig::hull()`.
#[derive(Clone, Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct ShipConfigData {
    /// Hull upgrade configs keyed by upgrade GameParam name (e.g. "PAUH442_New_York_1934").
    pub hull_upgrades: HashMap<String, HullUpgradeConfig>,
    /// Max main battery range across all artillery upgrades.
    pub main_battery_m: Option<Meters>,
    /// Max secondary battery range across all hull upgrades.
    pub secondary_battery_m: Option<Meters>,
    /// Names of torpedo ammo GameParams across all torpedo upgrades.
    /// Resolved to max range in meters in resolve_ranges().
    pub torpedo_ammo: HashSet<String>,
    /// Names of main battery ammo GameParams across all artillery upgrades.
    pub main_battery_ammo: HashSet<String>,
}

/// Resolved ship range values in real-world units.
/// Detection is in km, all weapon/consumable ranges are in meters.
#[derive(Clone, Debug, Default)]
pub struct ShipRanges {
    /// Sea detection range in km.
    pub detection_km: Option<Km>,
    /// Air detection range in km.
    pub air_detection_km: Option<Km>,
    /// Main battery max range in meters.
    pub main_battery_m: Option<Meters>,
    /// Secondary battery max range in meters.
    pub secondary_battery_m: Option<Meters>,
    /// Torpedo max range in meters.
    pub torpedo_range_m: Option<Meters>,
    /// Radar detection range in meters.
    pub radar_m: Option<Meters>,
    /// Hydro detection range in meters.
    pub hydro_m: Option<Meters>,
}

/// A hit location group on a ship hull (e.g. Bow, Citadel, Stern).
#[derive(Clone, Builder, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct HitLocation {
    max_hp: f32,
    hl_type: String,
    regenerated_hp_part: f32,
    /// Default armor plating thickness for this zone, in mm.
    #[cfg_attr(feature = "serde", serde(default))]
    thickness: f32,
    #[cfg_attr(feature = "serde", serde(default))]
    splash_boxes: Vec<String>,
}

impl HitLocation {
    pub fn max_hp(&self) -> f32 {
        self.max_hp
    }

    pub fn hl_type(&self) -> &str {
        &self.hl_type
    }

    pub fn regenerated_hp_part(&self) -> f32 {
        self.regenerated_hp_part
    }

    /// Default armor plating thickness for this zone, in mm.
    pub fn thickness(&self) -> f32 {
        self.thickness
    }

    pub fn splash_boxes(&self) -> &[String] {
        &self.splash_boxes
    }
}

#[derive(Clone, Builder, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct Vehicle {
    level: u32,
    group: String,
    abilities: Option<Vec<Vec<(String, String)>>>,
    #[cfg_attr(feature = "serde", serde(default))]
    upgrades: Vec<String>,
    #[cfg_attr(feature = "serde", serde(default))]
    config_data: Option<ShipConfigData>,
    #[cfg_attr(feature = "serde", serde(default))]
    model_path: Option<String>,
    #[cfg_attr(feature = "serde", serde(default))]
    armor: Option<ArmorMap>,
    #[cfg_attr(feature = "serde", serde(default))]
    hit_locations: Option<HashMap<String, HitLocation>>,
    #[cfg_attr(feature = "serde", serde(default))]
    permoflages: Vec<String>,
}

impl Vehicle {
    pub fn level(&self) -> u32 {
        self.level
    }

    pub fn group(&self) -> &str {
        self.group.as_ref()
    }

    pub fn abilities(&self) -> Option<&[Vec<(String, String)>]> {
        self.abilities.as_deref()
    }

    pub fn upgrades(&self) -> &[String] {
        self.upgrades.as_slice()
    }

    pub fn config_data(&self) -> Option<&ShipConfigData> {
        self.config_data.as_ref()
    }

    pub fn model_path(&self) -> Option<&str> {
        self.model_path.as_deref()
    }

    pub fn armor(&self) -> Option<&ArmorMap> {
        self.armor.as_ref()
    }

    pub fn hit_locations(&self) -> Option<&HashMap<String, HitLocation>> {
        self.hit_locations.as_ref()
    }

    pub fn permoflages(&self) -> &[String] {
        &self.permoflages
    }

    /// Look up a specific hull upgrade config by name.
    pub fn hull_upgrade(&self, name: &str) -> Option<&HullUpgradeConfig> {
        self.config_data.as_ref()?.hull_upgrades.get(name)
    }

    /// Get all hull upgrade configs, keyed by upgrade name.
    pub fn hull_upgrades(&self) -> Option<&HashMap<String, HullUpgradeConfig>> {
        self.config_data.as_ref().map(|c| &c.hull_upgrades)
    }

    /// Get the hull model path for a specific upgrade.
    pub fn model_path_for_hull(&self, upgrade_name: &str) -> Option<&str> {
        self.hull_upgrade(upgrade_name)?.hull_model_path()
    }

    /// Get all mount points for a specific hull upgrade.
    pub fn mounts_for_hull(&self, upgrade_name: &str) -> Vec<&MountPoint> {
        self.hull_upgrade(upgrade_name).map(|c| c.all_mount_points().collect()).unwrap_or_default()
    }

    /// Resolve the ship's ranges for a specific hull upgrade.
    ///
    /// `hull_name` is the GameParam name of the equipped hull upgrade.
    /// Look it up from the hull ID via `GameParamProvider::game_param_by_id()`.
    /// If `None`, the first available hull config is used as a fallback.
    ///
    /// Radar and hydro ranges are looked up from the ship's ability slots via
    /// `game_params`. Pass `None` to skip consumable range resolution.
    pub fn resolve_ranges(
        &self,
        game_params: Option<&dyn GameParamProvider>,
        hull_name: Option<&str>,
        version: crate::data::Version,
    ) -> ShipRanges {
        let mut ranges = ShipRanges::default();

        if let Some(config) = &self.config_data {
            let hull_config = hull_name
                .and_then(|name| config.hull_upgrades.get(name))
                .or_else(|| config.hull_upgrades.values().next());
            if let Some(hc) = hull_config {
                ranges.detection_km = Some(hc.detection_km);
                ranges.air_detection_km = Some(hc.air_detection_km);
                ranges.main_battery_m = config.main_battery_m;
                ranges.secondary_battery_m = config.secondary_battery_m;
                // Torpedo range: look up all ammo params and take the max range
                if let Some(game_params) = game_params {
                    let mut max_range: Option<Meters> = None;
                    for ammo_name in &config.torpedo_ammo {
                        if let Some(ammo_param) = game_params.game_param_by_name(ammo_name)
                            && let Some(projectile) = ammo_param.projectile()
                            && let Some(dist) = projectile.max_dist()
                        {
                            let m = dist.to_meters();
                            max_range = Some(match max_range {
                                Some(prev) if prev.value() >= m.value() => prev,
                                _ => m,
                            });
                        }
                    }
                    ranges.torpedo_range_m = max_range;
                }
            }
        }

        // Radar and hydro from consumable abilities
        if let (Some(game_params), Some(abilities)) = (game_params, &self.abilities) {
            for slot in abilities {
                for (ability_name, variant_name) in slot {
                    let param = match game_params.game_param_by_name(ability_name) {
                        Some(p) => p,
                        None => continue,
                    };
                    let ability = match param.ability() {
                        Some(a) => a,
                        None => continue,
                    };
                    let cat = match ability.get_category(variant_name) {
                        Some(c) => c,
                        None => continue,
                    };
                    match cat.consumable_type(version).known() {
                        Some(&crate::game_types::Consumable::Radar) => {
                            ranges.radar_m = cat.detection_radius();
                        }
                        Some(&crate::game_types::Consumable::HydroacousticSearch) => {
                            ranges.hydro_m = cat.detection_radius();
                        }
                        _ => {}
                    }
                }
            }
        }

        ranges
    }
}

#[derive(Clone, Builder, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct AbilityCategory {
    special_sound_id: Option<String>,
    consumable_type: String,
    description_id: String,
    group: String,
    icon_id: String,
    num_consumables: isize,
    preparation_time: f32,
    reload_time: f32,
    title_id: String,
    work_time: f32,
    /// Detection radius for ships (radar, hydro, sublocator). BigWorld units.
    #[cfg_attr(feature = "serde", serde(default))]
    dist_ship: Option<BigWorldDistance>,
    /// Detection radius for torpedoes (hydro only). BigWorld units.
    #[cfg_attr(feature = "serde", serde(default))]
    dist_torpedo: Option<BigWorldDistance>,
    /// Hydrophone wave radius in meters.
    #[cfg_attr(feature = "serde", serde(default))]
    hydrophone_wave_radius: Option<Meters>,
    /// Fighter patrol radius. BigWorld units.
    #[cfg_attr(feature = "serde", serde(default))]
    patrol_radius: Option<BigWorldDistance>,
}

impl AbilityCategory {
    pub fn consumable_type_raw(&self) -> &str {
        &self.consumable_type
    }

    pub fn consumable_type(
        &self,
        version: crate::data::Version,
    ) -> crate::recognized::Recognized<crate::game_types::Consumable> {
        crate::game_types::Consumable::from_consumable_type(&self.consumable_type, version)
    }

    pub fn icon_id(&self) -> &str {
        &self.icon_id
    }

    pub fn work_time(&self) -> f32 {
        self.work_time
    }

    /// Detection radius in meters.
    ///
    /// Returns hydrophone_wave_radius if present, otherwise converts dist_ship
    /// from BigWorld units to meters. Returns None if this consumable has no
    /// detection radius.
    pub fn detection_radius(&self) -> Option<Meters> {
        self.hydrophone_wave_radius.or_else(|| self.dist_ship.map(|d| d.to_meters()))
    }

    /// Torpedo detection radius in meters.
    pub fn torpedo_detection_radius(&self) -> Option<Meters> {
        self.dist_torpedo.map(|d| d.to_meters())
    }

    /// Fighter patrol radius in meters.
    pub fn patrol_radius(&self) -> Option<Meters> {
        self.patrol_radius.map(|d| d.to_meters())
    }
}

#[derive(Clone, Builder, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct Ability {
    can_buy: bool,
    cost_credits: isize,
    cost_gold: isize,
    is_free: bool,
    categories: HashMap<String, AbilityCategory>,
}

impl Ability {
    pub fn categories(&self) -> &HashMap<String, AbilityCategory> {
        &self.categories
    }

    pub fn get_category(&self, name: &str) -> Option<&AbilityCategory> {
        self.categories.get(name)
    }
}

#[derive(Clone, Builder, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct CrewPersonalityShips {
    groups: Vec<String>,
    nation: Vec<String>,
    peculiarity: Vec<String>,
    ships: Vec<String>,
}

#[derive(Clone, Builder, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct CrewPersonality {
    can_reset_skills_for_free: bool,
    cost_credits: usize,
    cost_elite_xp: usize,
    cost_gold: usize,
    cost_xp: usize,
    has_custom_background: bool,
    has_overlay: bool,
    has_rank: bool,
    has_sample_voiceover: bool,
    is_animated: bool,
    is_person: bool,
    is_retrainable: bool,
    is_unique: bool,
    peculiarity: String,
    /// TODO: flags?
    permissions: u32,
    person_name: String,
    ships: CrewPersonalityShips,
    subnation: String,
    tags: Vec<String>,
}

#[derive(Clone, Builder, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct ConsumableReloadTimeModifier {
    aircraft_carrier: f32,
    auxiliary: f32,
    battleship: f32,
    cruiser: f32,
    destroyer: f32,
    submarine: f32,
}

impl ConsumableReloadTimeModifier {
    pub fn get_for_species(&self, species: Species) -> f32 {
        match species {
            Species::AirCarrier => self.aircraft_carrier,
            Species::Battleship => self.battleship,
            Species::Cruiser => self.cruiser,
            Species::Destroyer => self.destroyer,
            Species::Submarine => self.submarine,
            Species::Auxiliary => self.auxiliary,
            other => panic!("Unexpected species {other:?}"),
        }
    }

    pub fn aircraft_carrier(&self) -> f32 {
        self.aircraft_carrier
    }

    pub fn auxiliary(&self) -> f32 {
        self.auxiliary
    }

    pub fn battleship(&self) -> f32 {
        self.battleship
    }

    pub fn cruiser(&self) -> f32 {
        self.cruiser
    }

    pub fn destroyer(&self) -> f32 {
        self.destroyer
    }

    pub fn submarine(&self) -> f32 {
        self.submarine
    }
}

#[derive(Clone, Builder, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct CrewSkillModifier {
    name: String,
    aircraft_carrier: f32,
    auxiliary: f32,
    battleship: f32,
    cruiser: f32,
    destroyer: f32,
    submarine: f32,
}

impl CrewSkillModifier {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn get_for_species(&self, species: &Species) -> f32 {
        match species {
            Species::AirCarrier => self.aircraft_carrier,
            Species::Battleship => self.battleship,
            Species::Cruiser => self.cruiser,
            Species::Destroyer => self.destroyer,
            Species::Submarine => self.submarine,
            Species::Auxiliary => self.auxiliary,
            _ => 1.0,
        }
    }
}

#[derive(Clone, Builder, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct CrewSkillLogicTrigger {
    /// Sometimes this field isn't present?
    burn_count: Option<usize>,
    change_priority_target_penalty: f32,
    consumable_type: String,
    cooling_delay: f32,
    /// TODO: figure out type
    cooling_interpolator: Vec<()>,
    divider_type: Option<String>,
    divider_value: Option<f32>,
    duration: f32,
    energy_coeff: f32,
    flood_count: Option<usize>,
    health_factor: Option<f32>,
    /// TODO: figure out type
    heat_interpolator: Vec<()>,
    modifiers: Option<Vec<CrewSkillModifier>>,
    trigger_desc_ids: String,
    trigger_type: String,
}

#[derive(Clone, Builder, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct CrewSkillTiers {
    aircraft_carrier: usize,
    auxiliary: usize,
    battleship: usize,
    cruiser: usize,
    destroyer: usize,
    submarine: usize,
}

impl CrewSkillTiers {
    pub fn get_for_species(&self, species: Species) -> usize {
        match species {
            Species::AirCarrier => self.aircraft_carrier,
            Species::Battleship => self.battleship,
            Species::Cruiser => self.cruiser,
            Species::Destroyer => self.destroyer,
            Species::Submarine => self.submarine,
            Species::Auxiliary => self.auxiliary,
            other => panic!("Unexpected species {other:?}"),
        }
    }

    pub fn aircraft_carrier(&self) -> usize {
        self.aircraft_carrier
    }

    pub fn auxiliary(&self) -> usize {
        self.auxiliary
    }

    pub fn battleship(&self) -> usize {
        self.battleship
    }

    pub fn cruiser(&self) -> usize {
        self.cruiser
    }

    pub fn destroyer(&self) -> usize {
        self.destroyer
    }

    pub fn submarine(&self) -> usize {
        self.submarine
    }
}

#[derive(Clone, Builder, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct CrewSkill {
    internal_name: String,
    logic_trigger: Option<CrewSkillLogicTrigger>,
    can_be_learned: bool,
    is_epic: bool,
    modifiers: Option<Vec<CrewSkillModifier>>,
    skill_type: usize,
    tier: CrewSkillTiers,
    ui_treat_as_trigger: bool,
}

impl CrewSkill {
    pub fn internal_name(&self) -> &str {
        self.internal_name.as_ref()
    }

    pub fn translated_name(&self, metadata_provider: &GameMetadataProvider) -> Option<String> {
        use convert_case::Case;
        use convert_case::Casing;
        let translation_id = format!("IDS_SKILL_{}", self.internal_name().to_case(Case::UpperSnake));

        metadata_provider.localized_name_from_id(&translation_id)
    }

    pub fn translated_description(&self, metadata_provider: &GameMetadataProvider) -> Option<String> {
        use convert_case::Case;
        use convert_case::Casing;
        let translation_id = format!("IDS_SKILL_DESC_{}", self.internal_name().to_case(Case::UpperSnake));

        let description = metadata_provider.localized_name_from_id(&translation_id);

        description.and_then(|desc| if desc.is_empty() || desc == " " { None } else { Some(desc) })
    }

    pub fn logic_trigger(&self) -> Option<&CrewSkillLogicTrigger> {
        self.logic_trigger.as_ref()
    }

    pub fn can_be_learned(&self) -> bool {
        self.can_be_learned
    }

    pub fn is_epic(&self) -> bool {
        self.is_epic
    }

    pub fn modifiers(&self) -> Option<&Vec<CrewSkillModifier>> {
        self.modifiers.as_ref()
    }

    pub fn skill_type(&self) -> usize {
        self.skill_type
    }

    pub fn tier(&self) -> &CrewSkillTiers {
        &self.tier
    }

    pub fn ui_treat_as_trigger(&self) -> bool {
        self.ui_treat_as_trigger
    }
}

#[derive(Clone, Builder, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct Crew {
    money_training_level: usize,
    personality: CrewPersonality,
    skills: Option<Vec<CrewSkill>>,
}

impl Crew {
    pub fn skill_by_type(&self, typ: u32) -> Option<&CrewSkill> {
        self.skills.as_ref().and_then(|skills| skills.iter().find(|skill| skill.skill_type == typ as usize))
    }

    pub fn skills(&self) -> Option<&[CrewSkill]> {
        self.skills.as_deref()
    }
}

#[derive(Clone, Builder, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct Achievement {
    is_group: bool,
    one_per_battle: bool,
    ui_type: String,
    ui_name: String,
}

impl Achievement {
    pub fn is_group(&self) -> bool {
        self.is_group
    }

    pub fn one_per_battle(&self) -> bool {
        self.one_per_battle
    }

    pub fn ui_type(&self) -> &str {
        &self.ui_type
    }

    pub fn ui_name(&self) -> &str {
        &self.ui_name
    }
}

/// Which icon directory a plane's icon should be loaded from.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub enum PlaneCategory {
    /// Catapult fighters, spotter planes
    Consumable,
    /// ASW depth-charge planes, mine-laying planes
    Airsupport,
    /// CV-controlled squadrons (default)
    #[default]
    Controllable,
}

#[derive(Clone, Builder, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct Aircraft {
    #[cfg_attr(feature = "serde", serde(default))]
    category: PlaneCategory,
    #[cfg_attr(feature = "serde", serde(default))]
    ammo_type: String,
}

impl Aircraft {
    pub fn category(&self) -> &PlaneCategory {
        &self.category
    }

    pub fn ammo_type(&self) -> &str {
        &self.ammo_type
    }
}

// ─── Shell / Ammo Types ─────────────────────────────────────────────────────────────

/// Strongly-typed ammunition type for projectiles.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum AmmoType {
    AP,
    HE,
    SAP,
    Unknown(String),
}

impl AmmoType {
    /// Parse from the game's internal string representation.
    pub fn from_game_str(s: &str) -> Self {
        match s {
            "AP" => Self::AP,
            "HE" => Self::HE,
            "CS" => Self::SAP,
            _ => Self::Unknown(s.to_string()),
        }
    }

    /// Display-friendly name.
    pub fn display_name(&self) -> &str {
        match self {
            Self::AP => "AP",
            Self::HE => "HE",
            Self::SAP => "SAP",
            Self::Unknown(s) => s,
        }
    }

    /// Sort order for consistent display (AP=0, HE=1, SAP=2, Unknown=3).
    pub fn sort_order(&self) -> u8 {
        match self {
            Self::AP => 0,
            Self::HE => 1,
            Self::SAP => 2,
            Self::Unknown(_) => 3,
        }
    }
}

impl std::fmt::Display for AmmoType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.display_name())
    }
}

/// Resolved shell information extracted from GameParams projectile data.
///
/// A flattened, easy-to-use representation of a `Projectile`'s ballistic
/// properties. All units are game-standard (mm for caliber/penetration,
/// m/s for velocity, kg for mass, degrees for angles).
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ShellInfo {
    pub name: String,
    pub ammo_type: AmmoType,
    pub caliber: Millimeters,
    pub he_pen_mm: Option<f32>,
    pub sap_pen_mm: Option<f32>,
    pub alpha_damage: f32,
    pub muzzle_velocity: f32,
    pub mass_kg: f32,
    pub krupp: f32,
    pub ricochet_angle: f32,
    pub always_ricochet_angle: f32,
    pub fuse_time: f32,
    pub fuse_threshold: f32,
    pub burn_prob: f32,
    pub air_drag: f32,
    pub normalization: f32,
}

#[derive(Clone, Builder, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct Projectile {
    #[cfg_attr(feature = "serde", serde(default))]
    ammo_type: String,
    /// Maximum range in BigWorld units. Present for torpedo ammo.
    #[cfg_attr(feature = "serde", serde(default))]
    max_dist: Option<BigWorldDistance>,
    /// Shell caliber in meters (bulletDiametr).
    #[cfg_attr(feature = "serde", serde(default))]
    bullet_diametr: Option<f32>,
    /// Shell mass in kg.
    #[cfg_attr(feature = "serde", serde(default))]
    bullet_mass: Option<f32>,
    /// Muzzle velocity in m/s.
    #[cfg_attr(feature = "serde", serde(default))]
    bullet_speed: Option<f32>,
    /// Krupp coefficient (AP penetration factor).
    #[cfg_attr(feature = "serde", serde(default))]
    bullet_krupp: Option<f32>,
    /// Whether the shell is capped (AP).
    #[cfg_attr(feature = "serde", serde(default))]
    bullet_cap: Option<bool>,
    /// Cap normalization max angle in degrees.
    #[cfg_attr(feature = "serde", serde(default))]
    bullet_cap_normalize_max_angle: Option<f32>,
    /// Fuse time in seconds.
    #[cfg_attr(feature = "serde", serde(default))]
    bullet_detonator: Option<f32>,
    /// Fuse threshold thickness in mm.
    #[cfg_attr(feature = "serde", serde(default))]
    bullet_detonator_threshold: Option<f32>,
    /// Ricochet start angle in degrees.
    #[cfg_attr(feature = "serde", serde(default))]
    bullet_ricochet_at: Option<f32>,
    /// Guaranteed ricochet angle in degrees.
    #[cfg_attr(feature = "serde", serde(default))]
    bullet_always_ricochet_at: Option<f32>,
    /// HE penetration in mm.
    #[cfg_attr(feature = "serde", serde(default))]
    alpha_piercing_he: Option<f32>,
    /// SAP penetration in mm.
    #[cfg_attr(feature = "serde", serde(default))]
    alpha_piercing_cs: Option<f32>,
    /// Alpha strike damage.
    #[cfg_attr(feature = "serde", serde(default))]
    alpha_damage: Option<f32>,
    /// Fire chance (-0.5 means N/A).
    #[cfg_attr(feature = "serde", serde(default))]
    burn_prob: Option<f32>,
    /// Air drag coefficient.
    #[cfg_attr(feature = "serde", serde(default))]
    bullet_air_drag: Option<f32>,
}

impl Projectile {
    pub fn ammo_type(&self) -> &str {
        &self.ammo_type
    }

    pub fn max_dist(&self) -> Option<BigWorldDistance> {
        self.max_dist
    }

    pub fn bullet_diametr(&self) -> Option<f32> {
        self.bullet_diametr
    }

    pub fn bullet_mass(&self) -> Option<f32> {
        self.bullet_mass
    }

    pub fn bullet_speed(&self) -> Option<f32> {
        self.bullet_speed
    }

    pub fn bullet_krupp(&self) -> Option<f32> {
        self.bullet_krupp
    }

    pub fn bullet_cap(&self) -> Option<bool> {
        self.bullet_cap
    }

    pub fn bullet_cap_normalize_max_angle(&self) -> Option<f32> {
        self.bullet_cap_normalize_max_angle
    }

    pub fn bullet_detonator(&self) -> Option<f32> {
        self.bullet_detonator
    }

    pub fn bullet_detonator_threshold(&self) -> Option<f32> {
        self.bullet_detonator_threshold
    }

    pub fn bullet_ricochet_at(&self) -> Option<f32> {
        self.bullet_ricochet_at
    }

    pub fn bullet_always_ricochet_at(&self) -> Option<f32> {
        self.bullet_always_ricochet_at
    }

    pub fn alpha_piercing_he(&self) -> Option<f32> {
        self.alpha_piercing_he
    }

    pub fn alpha_piercing_cs(&self) -> Option<f32> {
        self.alpha_piercing_cs
    }

    pub fn alpha_damage(&self) -> Option<f32> {
        self.alpha_damage
    }

    pub fn burn_prob(&self) -> Option<f32> {
        self.burn_prob
    }

    pub fn bullet_air_drag(&self) -> Option<f32> {
        self.bullet_air_drag
    }

    /// Convert this projectile to a [`ShellInfo`] with the given name.
    ///
    /// This is the canonical way to build a `ShellInfo` from game data,
    /// avoiding duplication across consumers.
    pub fn to_shell_info(&self, name: String) -> ShellInfo {
        let caliber_mm = Millimeters::new(self.bullet_diametr.unwrap_or(0.0) * 1000.0);
        ShellInfo {
            name,
            ammo_type: AmmoType::from_game_str(&self.ammo_type),
            caliber: caliber_mm,
            he_pen_mm: self.alpha_piercing_he,
            sap_pen_mm: self.alpha_piercing_cs,
            alpha_damage: self.alpha_damage.unwrap_or(0.0),
            muzzle_velocity: self.bullet_speed.unwrap_or(0.0),
            mass_kg: self.bullet_mass.unwrap_or(0.0),
            krupp: self.bullet_krupp.unwrap_or(0.0),
            ricochet_angle: self.bullet_ricochet_at.unwrap_or(45.0),
            always_ricochet_angle: self.bullet_always_ricochet_at.unwrap_or(60.0),
            fuse_time: self.bullet_detonator.unwrap_or(0.033),
            fuse_threshold: self.bullet_detonator_threshold.unwrap_or(0.0),
            burn_prob: self.burn_prob.unwrap_or(-0.5),
            air_drag: self.bullet_air_drag.unwrap_or(0.0),
            normalization: self.bullet_cap_normalize_max_angle.unwrap_or(0.0),
        }
    }
}

#[derive(Clone, Debug, Builder)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct BuffDrop {
    #[cfg_attr(feature = "serde", serde(default))]
    marker_name_active: String,
    #[cfg_attr(feature = "serde", serde(default))]
    marker_name_inactive: String,
    #[cfg_attr(feature = "serde", serde(default))]
    sorting: i64,
}

impl BuffDrop {
    pub fn marker_name_active(&self) -> &str {
        &self.marker_name_active
    }

    pub fn marker_name_inactive(&self) -> &str {
        &self.marker_name_inactive
    }

    pub fn sorting(&self) -> i64 {
        self.sorting
    }

    /// Returns the game asset path for the active icon.
    pub fn active_icon_path(&self) -> String {
        format!("gui/powerups/drops/icon_marker_{}.png", self.marker_name_active)
    }

    /// Returns the game asset path for the inactive icon.
    pub fn inactive_icon_path(&self) -> String {
        format!("gui/powerups/drops/icon_marker_{}.png", self.marker_name_inactive)
    }
}

#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct Modernization {
    modifiers: Vec<CrewSkillModifier>,
}

impl Modernization {
    pub fn new(modifiers: Vec<CrewSkillModifier>) -> Self {
        Self { modifiers }
    }

    pub fn modifiers(&self) -> &[CrewSkillModifier] {
        &self.modifiers
    }
}
#[derive(Clone, Debug, Variantly)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub enum ParamData {
    Vehicle(Vehicle),
    Crew(Crew),
    Ability(Ability),
    Achievement(Achievement),
    Modernization(Modernization),
    Exterior(Exterior),
    Unit,
    Aircraft(Aircraft),
    Projectile(Projectile),
    Drop(BuffDrop),
}

pub trait GameParamProvider {
    fn game_param_by_id(&self, id: GameParamId) -> Option<Rc<Param>>;
    fn game_param_by_index(&self, index: &str) -> Option<Rc<Param>>;
    fn game_param_by_name(&self, name: &str) -> Option<Rc<Param>>;
    fn params(&self) -> &[Rc<Param>];
}

#[derive(Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct GameParams {
    params: Vec<Rc<Param>>,
    #[cfg_attr(feature = "serde", serde(skip))]
    #[cfg_attr(feature = "rkyv", rkyv(with = rkyv::with::Skip))]
    id_to_params: HashMap<GameParamId, Rc<Param>>,
    #[cfg_attr(feature = "serde", serde(skip))]
    #[cfg_attr(feature = "rkyv", rkyv(with = rkyv::with::Skip))]
    index_to_params: HashMap<String, Rc<Param>>,
    #[cfg_attr(feature = "serde", serde(skip))]
    #[cfg_attr(feature = "rkyv", rkyv(with = rkyv::with::Skip))]
    name_to_params: HashMap<String, Rc<Param>>,
}

impl GameParamProvider for GameParams {
    fn game_param_by_id(&self, id: GameParamId) -> Option<Rc<Param>> {
        self.id_to_params.get(&id).cloned()
    }

    fn game_param_by_index(&self, index: &str) -> Option<Rc<Param>> {
        self.index_to_params.get(index).cloned()
    }

    fn game_param_by_name(&self, name: &str) -> Option<Rc<Param>> {
        self.name_to_params.get(name).cloned()
    }

    fn params(&self) -> &[Rc<Param>] {
        self.params.as_slice()
    }
}

struct ParamLookups {
    by_id: HashMap<GameParamId, Rc<Param>>,
    by_index: HashMap<String, Rc<Param>>,
    by_name: HashMap<String, Rc<Param>>,
}

fn build_param_lookups(params: &[Rc<Param>]) -> ParamLookups {
    let mut by_id = HashMap::with_capacity(params.len());
    let mut by_index = HashMap::with_capacity(params.len());
    let mut by_name = HashMap::with_capacity(params.len());
    for param in params {
        by_id.insert(param.id, param.clone());
        by_index.insert(param.index.clone(), param.clone());
        by_name.insert(param.name.clone(), param.clone());
    }

    ParamLookups { by_id, by_index, by_name }
}

impl<I> From<I> for GameParams
where
    I: IntoIterator<Item = Param>,
{
    fn from(value: I) -> Self {
        let params: Vec<Rc<Param>> = value.into_iter().map(Rc::new).collect();
        let lookups = build_param_lookups(params.as_ref());

        Self { params, id_to_params: lookups.by_id, index_to_params: lookups.by_index, name_to_params: lookups.by_name }
    }
}
