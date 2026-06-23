use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::HashSet;

use bon::Builder;

use crate::Rc;
use crate::data::ResourceLoader;
use crate::data::TranslationKey;
use crate::data::Version;
use crate::game_types::GameParamId;
use crate::game_types::GunId;
use crate::game_types::Vec2;
use crate::game_types::Vec3;

/// Friendly game version (major, minor, patch) at which the captain-skill rework
/// landed. Before this, a captain's learned skills are a single `learnedSkills`
/// UINT64 bitmask (1-indexed over skill types) and skill strings key as
/// `IDS_SKILL_<UPPERCASE>`; from this version on, learned skills are per-species
/// arrays of skill-type ids and strings key as `IDS_SKILL_<UPPER_SNAKE>`.
/// Verified empirically across 0.9.0-0.9.12 (bitmask) vs 0.10.0+ (arrays).
pub const CAPTAIN_SKILL_REWORK_VERSION: (u32, u32, u32) = (0, 10, 0);

use super::provider::GameMetadataProvider;

/// Distance newtypes. Defined in `wows-core`; re-exported so existing
/// `wowsunpack::game_params::types::{Meters, ...}` paths keep working.
pub use wows_core::units::BigWorldDistance;
/// Distance newtypes. Defined in `wows-core`; re-exported so existing
/// `wowsunpack::game_params::types::{Meters, ...}` paths keep working.
pub use wows_core::units::Km;
/// Distance newtypes. Defined in `wows-core`; re-exported so existing
/// `wowsunpack::game_params::types::{Meters, ...}` paths keep working.
pub use wows_core::units::Meters;
/// Distance newtypes. Defined in `wows-core`; re-exported so existing
/// `wowsunpack::game_params::types::{Meters, ...}` paths keep working.
pub use wows_core::units::MetersPerSecond;
/// Distance newtypes. Defined in `wows-core`; re-exported so existing
/// `wowsunpack::game_params::types::{Meters, ...}` paths keep working.
pub use wows_core::units::Millimeters;
/// Distance newtypes. Defined in `wows-core`; re-exported so existing
/// `wowsunpack::game_params::types::{Meters, ...}` paths keep working.
pub use wows_core::units::ShipModelDistance;

/// Per-material armor thickness map.
///
/// Outer key = collision material ID (0-254).
/// Inner key = model_index (1-based armor model ordinal).
/// Value = thickness in mm for that model_index.
///
/// The game registers the same geometry multiple times with different
/// `model_index` values; each registration represents a separate armor layer.
pub type ArmorMap = HashMap<u32, BTreeMap<u32, f32>>;

/// A captain-skill type id (the client's `SkillTypeEnum` ordinal). This is a
/// per-version ordinal used for same-version equality matching; the stable
/// identity is the skill's name (CrewSkillName).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct CrewSkillType(u32);

impl CrewSkillType {
    pub fn new(value: u32) -> Self {
        Self(value)
    }

    pub fn raw(self) -> u32 {
        self.0
    }
}

impl From<u8> for CrewSkillType {
    fn from(v: u8) -> Self {
        Self(v as u32)
    }
}

impl From<u32> for CrewSkillType {
    fn from(v: u32) -> Self {
        Self(v)
    }
}

/// A captain-skill's stable string identity (the client's `SkillTypeEnum` name,
/// e.g. `TriggerSpreading`). Pairs with `CrewSkillType`: a string identity and a
/// numeric identity for the same skill.
#[derive(Debug, Default, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct CrewSkillName(String);

impl CrewSkillName {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<String> for CrewSkillName {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for CrewSkillName {
    fn from(s: &str) -> Self {
        Self(s.to_owned())
    }
}

impl std::fmt::Display for CrewSkillName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// A captain-skill point cost (1-based: skills cost 1-4 points). Distinct from a
/// 0-based grid row index, which never escapes the grid table.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct SkillPointCost(u8);

impl SkillPointCost {
    pub fn new(cost: u8) -> Self {
        Self(cost)
    }

    /// Convert a 0-based grid row index to its 1-based point cost. Rows are 0-3
    /// in real data; `row` must be < 255.
    pub fn from_grid_row(row: u8) -> Self {
        Self(row + 1)
    }

    pub fn get(self) -> u8 {
        self.0
    }
}

/// Captain skills recognized by internal_name; Unknown covers future/unseen skills.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KnownCrewSkill {
    IncomingFireAlert,
    Dazzle,
    InertiaFuse,
    MainBatteryAndAaSpecialist,
    BasicsOfSurvivability,
    GreaseTheGears,
    FillTheTubes,
    EmergencyRepairSpecialist,
    ConsumableEnhancements,
    Vigilance,
    DemolitionExpert,
    MainBatteryAndAaExpert,
    AircraftArmor,
    ImprovedEngineBoost,
    ConcealmentExpert,
    ConsumableSpecialist,
    FirePreventionExpert,
    SightStabilization,
    ImprovedEngines,
    Superintendent,
    PreventiveMaintenance,
    LastStand,
    GunFeeder,
    Interceptor,
    AdrenalineRush,
    SwiftFish,
    SurvivabilityExpert,
    ManualSecondaryBatteryAiming,
    FocusFireTraining,
    PriorityTarget,
    AirSupremacy,
    PackAPunch,
    DirectionCenterForFighters,
    EngineTechie,
    RadioLocation,
    AaDefenseAndAswExpert,
    SecondaryArmamentExpert,
    SuperHeavyApShells,
    HeavyApShells,
    ExtraHeavyAmmunition,
    LongRangeSecondaryBatteryShells,
    DefensiveFireExpert,
    EmergencyRepairExpert,
    EyeInTheSky,
    ImprovedRepairPartyReadiness,
    HiddenMenace,
    Pyrotechnician,
    HeavyHeAndSapShells,
    EnhancedArmorPiercingAmmunition,
    PatrolGroupLeader,
    EnhancedReactions,
    SearchAndDestroy,
    RepairSpecialist,
    EnhancedAircraftArmor,
    BomberFlightControl,
    LastGasp,
    SurvivabilityExpertAircraft,
    TorpedoBomber,
    SwiftFlyingFish,
    ProximityFuze,
    Liquidator,
    Brisk,
    CloseQuartersCombat,
    TopGradeGunner,
    FearlessBrawler,
    SwiftInSilence,
    Outnumbered,
    EnhancedSonar,
    EnhancedImpulseGenerator,
    Sonarman,
    SonarmanExpert,
    TorpedoCrewTraining,
    TorpedoAimingMaster,
    Helmsman,
    ImprovedBatteryCapacity,
    Watchful,
    ImprovedBatteryEfficiency,
    EnlargedPropellerShaft,
    SubmarineConsumableSpecialist,
    SubmarineConsumableEnhancements,
    Furious,
    SubmarineAdrenalineRush,
    PlanesActiveManeuvering,
}

impl KnownCrewSkill {
    pub fn recognize(
        name: &CrewSkillName,
        raw: CrewSkillType,
    ) -> crate::recognized::Recognized<KnownCrewSkill, CrewSkillType> {
        use crate::recognized::Recognized::Known;
        match name.as_str() {
            "GmReloadAaDamageConstant" => Known(KnownCrewSkill::MainBatteryAndAaSpecialist),
            "DefenceCritFireFlooding" => Known(KnownCrewSkill::BasicsOfSurvivability),
            "GmTurn" => Known(KnownCrewSkill::GreaseTheGears),
            "TorpedoReload" => Known(KnownCrewSkill::FillTheTubes),
            "ConsumablesCrashcrewRegencrewReload" => Known(KnownCrewSkill::EmergencyRepairSpecialist),
            "ConsumablesDuration" => Known(KnownCrewSkill::ConsumableEnhancements),
            "DetectionTorpedoRange" => Known(KnownCrewSkill::Vigilance),
            "HeFireProbability" => Known(KnownCrewSkill::DemolitionExpert),
            "GmRangeAaDamageBubbles" => Known(KnownCrewSkill::MainBatteryAndAaExpert),
            "PlanesDefenseDamageConstant" => Known(KnownCrewSkill::AircraftArmor),
            "PlanesForsageDuration" => Known(KnownCrewSkill::ImprovedEngineBoost),
            "DetectionVisibilityRange" => Known(KnownCrewSkill::ConcealmentExpert),
            "ConsumablesReload" => Known(KnownCrewSkill::ConsumableSpecialist),
            "DefenceFireProbability" => Known(KnownCrewSkill::FirePreventionExpert),
            "PlanesAimingBoost" => Known(KnownCrewSkill::SightStabilization),
            "PlanesSpeed" => Known(KnownCrewSkill::ImprovedEngines),
            "ConsumablesAdditional" => Known(KnownCrewSkill::Superintendent),
            "DefenseCritProbability" => Known(KnownCrewSkill::PreventiveMaintenance),
            "DetectionAlert" => Known(KnownCrewSkill::IncomingFireAlert),
            "Maneuverability" => Known(KnownCrewSkill::LastStand),
            "GmShellReload" => Known(KnownCrewSkill::GunFeeder),
            "PlanesConsumablesCallfightersUpgrade" => Known(KnownCrewSkill::Interceptor),
            "ArmamentReloadAaDamage" => Known(KnownCrewSkill::AdrenalineRush),
            "TorpedoSpeed" => Known(KnownCrewSkill::SwiftFish),
            "DefenseHp" => Known(KnownCrewSkill::SurvivabilityExpert),
            "AtbaAccuracy" => Known(KnownCrewSkill::ManualSecondaryBatteryAiming),
            "AaPrioritysectorDamageConstant" => Known(KnownCrewSkill::FocusFireTraining),
            "DetectionAiming" => Known(KnownCrewSkill::PriorityTarget),
            "PlanesReload" => Known(KnownCrewSkill::AirSupremacy),
            "TorpedoDamage" => Known(KnownCrewSkill::PackAPunch),
            "ConsumablesFighterAdditional" => Known(KnownCrewSkill::DirectionCenterForFighters),
            "PlanesConsumablesSpeedboosterReload" => Known(KnownCrewSkill::EngineTechie),
            "HePenetration" => Known(KnownCrewSkill::InertiaFuse),
            "DetectionDirection" => Known(KnownCrewSkill::RadioLocation),
            "AaDamageConstantBubbles" => Known(KnownCrewSkill::AaDefenseAndAswExpert),
            "AaDamageConstantBubblesCv" => Known(KnownCrewSkill::SecondaryArmamentExpert),
            "ApDamageBb" => Known(KnownCrewSkill::SuperHeavyApShells),
            "ApDamageCa" => Known(KnownCrewSkill::HeavyApShells),
            "ApDamageDd" => Known(KnownCrewSkill::ExtraHeavyAmmunition),
            "AtbaRange" => Known(KnownCrewSkill::LongRangeSecondaryBatteryShells),
            "AtbaUpgrade" => Known(KnownCrewSkill::DefensiveFireExpert),
            "ConsumablesCrashcrewRegencrewUpgrade" => Known(KnownCrewSkill::EmergencyRepairExpert),
            "ConsumablesSpotterUpgrade" => Known(KnownCrewSkill::EyeInTheSky),
            "DefenceUw" => Known(KnownCrewSkill::ImprovedRepairPartyReadiness),
            "DetectionVisibilityCrashcrew" => Known(KnownCrewSkill::HiddenMenace),
            "HeFireProbabilityCv" => Known(KnownCrewSkill::Pyrotechnician),
            "HeSapDamage" => Known(KnownCrewSkill::HeavyHeAndSapShells),
            "PlanesApDamage" => Known(KnownCrewSkill::EnhancedArmorPiercingAmmunition),
            "PlanesConsumablesCallfightersAdditional" => Known(KnownCrewSkill::PatrolGroupLeader),
            "PlanesConsumablesCallfightersPreparationtime" => Known(KnownCrewSkill::EnhancedReactions),
            "PlanesConsumablesCallfightersRange" => Known(KnownCrewSkill::SearchAndDestroy),
            "PlanesConsumablesRegeneratehealthUpgrade" => Known(KnownCrewSkill::RepairSpecialist),
            "PlanesDefenseDamageBubbles" => Known(KnownCrewSkill::EnhancedAircraftArmor),
            "PlanesDivebomberSpeed" => Known(KnownCrewSkill::BomberFlightControl),
            "PlanesForsageRenewal" => Known(KnownCrewSkill::LastGasp),
            "PlanesHp" => Known(KnownCrewSkill::SurvivabilityExpertAircraft),
            "PlanesTorpedoArmingrange" => Known(KnownCrewSkill::TorpedoBomber),
            "PlanesTorpedoSpeed" => Known(KnownCrewSkill::SwiftFlyingFish),
            "PlanesTorpedoUwReduced" => Known(KnownCrewSkill::ProximityFuze),
            "TorpedoFloodingProbability" => Known(KnownCrewSkill::Liquidator),
            "TriggerSpeedBb" => Known(KnownCrewSkill::Brisk),
            "TriggerGmAtbaReloadBb" => Known(KnownCrewSkill::CloseQuartersCombat),
            "TriggerGmAtbaReloadCa" => Known(KnownCrewSkill::TopGradeGunner),
            "TriggerGmReload" => Known(KnownCrewSkill::FearlessBrawler),
            "TriggerSpeed" => Known(KnownCrewSkill::SwiftInSilence),
            "TriggerSpeedAccuracy" => Known(KnownCrewSkill::Outnumbered),
            "TriggerSpreading" => Known(KnownCrewSkill::Dazzle),
            "TriggerPingerReloadBuff" => Known(KnownCrewSkill::EnhancedSonar),
            "TriggerPingerSpeedBuff" => Known(KnownCrewSkill::EnhancedImpulseGenerator),
            "SubmarineHoldSectors" => Known(KnownCrewSkill::Sonarman),
            "TriggerConsSonarTimeCoeff" => Known(KnownCrewSkill::SonarmanExpert),
            "TriggerSeenTorpedoReload" => Known(KnownCrewSkill::TorpedoCrewTraining),
            "SubmarineTorpedoPingDamage" => Known(KnownCrewSkill::TorpedoAimingMaster),
            "TriggerConsRudderTimeCoeff" => Known(KnownCrewSkill::Helmsman),
            "SubmarineBatteryCapacity" => Known(KnownCrewSkill::ImprovedBatteryCapacity),
            "SubmarineDangerAlert" => Known(KnownCrewSkill::Watchful),
            "SubmarineBatteryBurnDown" => Known(KnownCrewSkill::ImprovedBatteryEfficiency),
            "SubmarineSpeed" => Known(KnownCrewSkill::EnlargedPropellerShaft),
            "SubmarineConsumablesReload" => Known(KnownCrewSkill::SubmarineConsumableSpecialist),
            "SubmarineConsumablesDuration" => Known(KnownCrewSkill::SubmarineConsumableEnhancements),
            "TriggerBurnGmReload" => Known(KnownCrewSkill::Furious),
            "ArmamentReloadSubmarine" => Known(KnownCrewSkill::SubmarineAdrenalineRush),
            "PlanesConsumablesActiveManeuveringUpgrade" => Known(KnownCrewSkill::PlanesActiveManeuvering),
            _ => crate::recognized::Recognized::Unknown(raw),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
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

    /// Returns the Building data if this param is a Building type.
    pub fn building(&self) -> Option<&Building> {
        match &self.data {
            ParamData::Building(b) => Some(b),
            _ => None,
        }
    }

    /// Returns the Unit data if this param is a Unit (ship module) type.
    pub fn unit(&self) -> Option<&Unit> {
        match &self.data {
            ParamData::Unit(u) => Some(u),
            _ => None,
        }
    }
}

#[derive(PartialEq, Eq, Hash, Debug, Clone, Copy)]
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
    /// Signal-flag modifiers (e.g. `speedCoef` on `PCEF005_SM_SignalFlag`).
    /// Empty for camos with no gameplay effect.
    #[cfg_attr(feature = "serde", serde(default))]
    modifiers: Vec<CrewSkillModifier>,
}

impl Exterior {
    pub fn camouflage(&self) -> Option<&str> {
        self.camouflage.as_deref()
    }

    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    pub fn modifiers(&self) -> &[CrewSkillModifier] {
        &self.modifiers
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
    /// Component type key to component name (e.g. "artillery" -> "AB1_Artillery").
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

/// One camera orbit trajectory from a ship's `Cameras` component.
/// Values are raw ship-model units, ship-local. `pos_center` holds the two
/// FOV-range endpoints; the active point is `pos_center[0].lerp(pos_center[1], fov)`.
/// `semi_axes` are the ellipse radii per endpoint: `.x` along the beam (model X),
/// `.y` along the length (model Z).
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct CameraTrajectory {
    pub pos_center: [Vec3; 2],
    pub semi_axes: [Vec2; 2],
    pub tags: String,
    pub ignore_height_multiplier: bool,
    /// The scrolled-out orbit, when the mode defines one. Carries the same
    /// FOV-endpoint layout as the inner trajectory and its own height gate;
    /// `tags` are shared from the parent mode.
    #[cfg_attr(feature = "serde", serde(default))]
    pub outer: Option<TrajectoryGeometry>,
}

/// Authored orbit geometry for one trajectory (inner or outer): both posCenter
/// FOV endpoints, the per-endpoint ellipse radii, and the height gate. Distinct
/// from [`CameraRing`], which is a single resolved ring.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct TrajectoryGeometry {
    pub pos_center: [Vec3; 2],
    pub semi_axes: [Vec2; 2],
    pub ignore_height_multiplier: bool,
}

/// Resolved camera ring for a specific FOV blend and height offset.
/// `semi_axes.x` is the beam (model X) radius, `.y` the length (model Z) radius.
#[derive(Debug, Clone, Copy)]
pub struct CameraRing {
    pub pos_center: Vec3,
    pub semi_axes: Vec2,
}

// From CameraConstants.py heightMultiplierTags / innerOnlyHeightMultiplierTags.
const HEIGHT_TAGS: &[u8] = b"AMTOPBb";
const INNER_ONLY_TAGS: &[u8] = b"BbP";
// INNER_HEIGHT_TRAJECTORY_COEF from m30c89f53.py. Approximated symmetrically.
const HEIGHT_COEF: f32 = 2.0;

impl CameraTrajectory {
    fn height_applies(&self, ignore_height_multiplier: bool) -> bool {
        !ignore_height_multiplier
            && self.tags.bytes().any(|c| HEIGHT_TAGS.contains(&c))
            && !self.tags.bytes().any(|c| INNER_ONLY_TAGS.contains(&c))
    }

    fn blend_ring(pos_center: &[Vec3; 2], semi_axes: &[Vec2; 2], fov: f32) -> CameraRing {
        let f = fov.clamp(0.0, 1.0);
        CameraRing { pos_center: pos_center[0].lerp(pos_center[1], f), semi_axes: semi_axes[0].lerp(semi_axes[1], f) }
    }

    /// FOV blend only (height = 0).
    pub fn ring(&self, fov: f32) -> CameraRing {
        Self::blend_ring(&self.pos_center, &self.semi_axes, fov)
    }

    /// FOV blend plus the game's gated height shift on posCenter.y.
    pub fn resolve(&self, fov: f32, height: f32) -> CameraRing {
        let mut r = self.ring(fov);
        if self.height_applies(self.ignore_height_multiplier) {
            r.pos_center.y += HEIGHT_COEF * height;
        }
        r
    }

    /// The outer orbit at the given FOV blend, when this mode defines one.
    pub fn outer_ring(&self, fov: f32) -> Option<CameraRing> {
        let o = self.outer.as_ref()?;
        Some(Self::blend_ring(&o.pos_center, &o.semi_axes, fov))
    }

    /// Outer orbit FOV blend plus the gated height shift, when this mode
    /// defines an outer trajectory.
    pub fn resolve_outer(&self, fov: f32, height: f32) -> Option<CameraRing> {
        let o = self.outer.as_ref()?;
        let mut r = self.outer_ring(fov)?;
        if self.height_applies(o.ignore_height_multiplier) {
            r.pos_center.y += HEIGHT_COEF * height;
        }
        Some(r)
    }
}

#[cfg(test)]
mod camera_ring_tests {
    use super::*;

    fn traj(tags: &str, ignore: bool) -> CameraTrajectory {
        CameraTrajectory {
            pos_center: [Vec3::new(0.0, 1.958, 0.0), Vec3::new(0.0, 2.008, 0.0)],
            semi_axes: [Vec2::new(6.552, 9.6), Vec2::new(5.981, 9.6)],
            tags: tags.to_string(),
            ignore_height_multiplier: ignore,
            outer: None,
        }
    }

    #[test]
    fn fov0_is_inner() {
        let r = traj("AT", false).resolve(0.0, 0.0);
        assert!((r.semi_axes.x - 6.552).abs() < 1e-4 && (r.pos_center.y - 1.958).abs() < 1e-4);
    }

    #[test]
    fn fov1_is_outer() {
        let r = traj("AT", false).resolve(1.0, 0.0);
        assert!((r.semi_axes.x - 5.981).abs() < 1e-4 && (r.pos_center.y - 2.008).abs() < 1e-4);
    }

    #[test]
    fn height_applies_for_artillery() {
        let r = traj("AT", false).resolve(0.0, 1.0);
        assert!((r.pos_center.y - (1.958 + 2.0)).abs() < 1e-4);
    }

    #[test]
    fn height_skipped_for_periscope() {
        let r = traj("B", false).resolve(0.0, 1.0);
        assert!((r.pos_center.y - 1.958).abs() < 1e-4);
    }

    #[test]
    fn height_skipped_when_ignored() {
        let r = traj("AT", true).resolve(0.0, 1.0);
        assert!((r.pos_center.y - 1.958).abs() < 1e-4);
    }

    fn traj_with_outer() -> CameraTrajectory {
        let mut t = traj("AT", false);
        t.outer = Some(TrajectoryGeometry {
            pos_center: [Vec3::new(0.0, 4.199, 0.0), Vec3::new(0.0, 6.023, 0.0)],
            semi_axes: [Vec2::new(18.785, 18.785), Vec2::new(17.315, 17.315)],
            ignore_height_multiplier: false,
        });
        t
    }

    #[test]
    fn resolve_outer_absent_returns_none() {
        assert!(traj("AT", false).resolve_outer(0.0, 0.0).is_none());
    }

    #[test]
    fn resolve_outer_fov0_uses_first_endpoint() {
        let r = traj_with_outer().resolve_outer(0.0, 0.0).expect("outer present");
        assert!((r.semi_axes.x - 18.785).abs() < 1e-4);
        assert!((r.pos_center.y - 4.199).abs() < 1e-4);
    }

    #[test]
    fn resolve_outer_fov1_uses_second_endpoint() {
        let r = traj_with_outer().resolve_outer(1.0, 0.0).expect("outer present");
        assert!((r.semi_axes.y - 17.315).abs() < 1e-4);
        assert!((r.pos_center.y - 6.023).abs() < 1e-4);
    }

    #[test]
    fn resolve_outer_height_applies_for_artillery() {
        let r = traj_with_outer().resolve_outer(0.0, 1.0).expect("outer present");
        assert!((r.pos_center.y - (4.199 + 2.0)).abs() < 1e-4);
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
    /// Names of secondary (ATBA) ammo GameParams across all hull upgrades.
    pub secondary_battery_ammo: HashSet<String>,
    /// Secondary (ATBA) ammo GameParam name per gun, ordered by hardpoint number
    /// (HP_GGS_<n>). Index == wire gunID under the assumption that the client's
    /// gun list follows hardpoint numeric order. Used for per-gun dot pacing;
    /// empty string marks a mount with no resolvable ammo so indices stay aligned.
    pub secondary_guns: Vec<String>,
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
    /// Camera orbit trajectories (per mode) from the ship `Cameras` component.
    #[cfg_attr(feature = "serde", serde(default))]
    camera_trajectories: Vec<(String, CameraTrajectory)>,
    /// Typed TTX component base stats (hull/engine) extracted at parse time.
    /// Query-time factories apply formulas + modifiers without the raw pickle.
    #[cfg_attr(feature = "serde", serde(default))]
    ttx_components: Option<super::ttx::components::ShipTtxComponents>,
    /// HP-breakpoint innate skills from the ship's `innateSkills` hull component.
    /// Empty for ships that have no innate skills.
    #[cfg_attr(feature = "serde", serde(default))]
    innate_skills: Vec<InnateSkill>,
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

    pub fn camera_trajectories(&self) -> &[(String, CameraTrajectory)] {
        &self.camera_trajectories
    }

    /// Typed TTX hull/engine base stats, if any component sub-objects resolved.
    pub fn ttx_components(&self) -> Option<&super::ttx::components::ShipTtxComponents> {
        self.ttx_components.as_ref()
    }

    /// HP-breakpoint innate skills from the ship's hull component.
    pub fn innate_skills(&self) -> &[InnateSkill] {
        &self.innate_skills
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
    group: String,
    icon_id: String,
    num_consumables: isize,
    preparation_time: f32,
    reload_time: f32,
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
    /// Repair Party heal rate as a fraction of max HP per second. `None` for
    /// non-heal consumables.
    #[cfg_attr(feature = "serde", serde(default))]
    regeneration_hp_speed: Option<f32>,
    /// Repair Party flat heal rate in HP per second (added to the fraction
    /// term). `None` for non-heal consumables.
    #[cfg_attr(feature = "serde", serde(default))]
    regeneration_hp_speed_units: Option<f32>,
    /// Raw numeric fields merged from the category root and its `logic`
    /// sub-dict (logic wins on collision), mirroring the client's generic
    /// consumable attribute extraction. Fed to the modifier engine for display.
    #[cfg_attr(feature = "serde", serde(default))]
    #[builder(default)]
    effect_fields: BTreeMap<String, f32>,
    /// Named modifiers from `logic.modifiers` as uniform per-species values.
    /// Empty for consumables whose logic sub-dict carries no `modifiers` block.
    /// Each entry corresponds to a modifier name (e.g. `GMIdealRadius`) with the
    /// same float value applied uniformly across all ship species.
    #[cfg_attr(feature = "serde", serde(default))]
    #[builder(default)]
    modifiers: Vec<CrewSkillModifier>,
}

impl AbilityCategory {
    pub fn consumable_type_raw(&self) -> &str {
        &self.consumable_type
    }

    /// The consumable group string, `"ship"` or `"squadron"` (client
    /// `ConsumableGroup`). Empty when absent.
    pub fn group(&self) -> &str {
        &self.group
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

    pub fn reload_time(&self) -> f32 {
        self.reload_time
    }

    pub fn preparation_time(&self) -> f32 {
        self.preparation_time
    }

    /// Raw `numConsumables` from GameParams. A value of `-1` means unlimited.
    /// Callers that want a typed result should wrap this via
    /// `wows_replay_insights::build::ChargeCount::from_game_params`.
    pub fn num_consumables(&self) -> isize {
        self.num_consumables
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

    /// Repair Party heal rate, fraction of max HP per second (RegenCrew only).
    pub fn regeneration_hp_speed(&self) -> Option<f32> {
        self.regeneration_hp_speed
    }

    /// Repair Party flat heal rate, HP per second (RegenCrew only).
    pub fn regeneration_hp_speed_units(&self) -> Option<f32> {
        self.regeneration_hp_speed_units
    }

    /// Raw numeric effect fields merged from the category root and `logic`.
    pub fn effect_fields(&self) -> &BTreeMap<String, f32> {
        &self.effect_fields
    }

    /// Named modifiers from `logic.modifiers` as uniform per-species values.
    /// Empty for consumables without a `modifiers` block in their logic sub-dict.
    pub(crate) fn modifiers(&self) -> &[CrewSkillModifier] {
        &self.modifiers
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
    has_custom_background: Option<bool>,
    has_overlay: Option<bool>,
    has_rank: Option<bool>,
    has_sample_voiceover: Option<bool>,
    is_animated: Option<bool>,
    is_person: Option<bool>,
    is_retrainable: Option<bool>,
    is_unique: Option<bool>,
    peculiarity: Option<String>,
    /// TODO: flags?
    permissions: Option<u32>,
    person_name: String,
    ships: CrewPersonalityShips,
    subnation: Option<String>,
    tags: Vec<String>,
}

impl CrewPersonality {
    /// True for named, one-off commanders (Halsey, Yamamoto, ...) versus the
    /// generic nation crews. Absent in the data means not unique.
    pub fn is_unique(&self) -> bool {
        self.is_unique.unwrap_or(false)
    }

    /// True when the commander is a real person (portrait + name) rather than a
    /// generic crew. Absent means false.
    pub fn is_person(&self) -> bool {
        self.is_person.unwrap_or(false)
    }

    /// The person-name token (e.g. "Halsey"). The localized display name is
    /// looked up as `IDS_<NAME>` with the token uppercased.
    pub fn name(&self) -> &str {
        &self.person_name
    }
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

#[derive(Clone, Builder, Debug, PartialEq)]
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
    /// Consumable type names (e.g. `"crashCrew"`, `"regenCrew"`) that this
    /// modifier does NOT apply to. Sourced from the `excludedConsumables`
    /// sibling key in the same modifier dict (e.g. the Survival Expert skill).
    /// Empty when the modifier applies universally.
    #[cfg_attr(feature = "serde", serde(default))]
    excluded_consumables: Vec<String>,
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

    pub fn excluded_consumables(&self) -> &[String] {
        &self.excluded_consumables
    }

    /// True when this modifier is suppressed for the given `consumable_type`
    /// (matched as the raw GameParams string, e.g. `"crashCrew"`).
    pub fn excludes(&self, consumable_type: &str) -> bool {
        self.excluded_consumables.iter().any(|t| t == consumable_type)
    }

    /// A modifier whose value is the same for every species. The effects engine builds
    /// these to carry an evaluated coefficient into `ModifierBundle::from_modifiers`,
    /// which reads only the resolved species.
    pub(crate) fn uniform(name: &str, value: f32) -> CrewSkillModifier {
        CrewSkillModifier::builder()
            .name(name.to_owned())
            .aircraft_carrier(value)
            .auxiliary(value)
            .battleship(value)
            .cruiser(value)
            .destroyer(value)
            .submarine(value)
            .excluded_consumables(Vec::new())
            .build()
    }
}

/// A piecewise-linear control-point ramp (`MultiLerp`): `(x, y)` points, `y` in 0..1.
/// Evaluation (the lerp) lands in the heat sub-project; this is the parsed shape.
#[derive(Clone, Debug, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct Interpolator(Vec<(f32, f32)>);

impl Interpolator {
    pub fn from_points(points: Vec<(f32, f32)>) -> Self {
        Interpolator(points)
    }

    pub fn points(&self) -> &[(f32, f32)] {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Clamped piecewise-linear evaluation (client `MultiLerp`, extrapolate=false): `x`
    /// at or below the first point's x returns the first y, at or above the last returns
    /// the last y, otherwise linear between the bracketing points. Empty -> 0.0.
    pub fn eval(&self, x: f32) -> f32 {
        let pts = &self.0;
        let Some(&(first_x, first_y)) = pts.first() else {
            return 0.0;
        };
        if x <= first_x {
            return first_y;
        }
        let &(last_x, last_y) = pts.last().unwrap();
        if x >= last_x {
            return last_y;
        }
        for w in pts.windows(2) {
            let (x0, y0) = w[0];
            let (x1, y1) = w[1];
            if x >= x0 && x <= x1 {
                if x1 == x0 {
                    return y0;
                }
                let t = (x - x0) / (x1 - x0);
                return y0 + (y1 - y0) * t;
            }
        }
        last_y
    }

    /// The last control point's x (the saturation input); 0.0 if empty.
    pub fn max_x(&self) -> f32 {
        self.0.last().map(|&(x, _)| x).unwrap_or(0.0)
    }
}

#[derive(Clone, Builder, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct CrewSkillLogicTrigger {
    burn_count: Option<usize>,
    change_priority_target_penalty: Option<f32>,
    consumable_type: String,
    cooling_delay: f32,
    cooling_interpolator: Interpolator,
    count_to_modifier: Vec<(u32, Vec<CrewSkillModifier>)>,
    damage_value: Option<f32>,
    divider_type: Option<String>,
    divider_value: Option<f32>,
    duration: f32,
    energy_coeff: f32,
    flood_count: Option<usize>,
    health_factor: Option<f32>,
    heat_interpolator: Interpolator,
    modifiers: Option<Vec<CrewSkillModifier>>,
    trigger_desc_ids: String,
    trigger_type: String,
}

impl CrewSkillLogicTrigger {
    pub fn modifiers(&self) -> Option<&Vec<CrewSkillModifier>> {
        self.modifiers.as_ref()
    }

    pub fn trigger_type(&self) -> &str {
        &self.trigger_type
    }

    pub fn damage_value(&self) -> Option<f32> {
        self.damage_value
    }

    pub fn count_to_modifier(&self) -> &[(u32, Vec<CrewSkillModifier>)] {
        &self.count_to_modifier
    }

    pub fn heat_interpolator(&self) -> &Interpolator {
        &self.heat_interpolator
    }

    pub fn cooling_interpolator(&self) -> &Interpolator {
        &self.cooling_interpolator
    }

    /// The consumable this trigger watches (`activationOnConsumable`), resolved to a type.
    pub fn consumable_type(
        &self,
        version: crate::data::Version,
    ) -> crate::recognized::Recognized<crate::game_types::Consumable> {
        crate::game_types::Consumable::from_consumable_type(&self.consumable_type, version)
    }

    /// The trigger's active-window duration in seconds (`activationOnDetect`/`activationOnConsumable`).
    pub fn duration(&self) -> f32 {
        self.duration
    }
}

/// One HP-fraction breakpoint from a ship's innate adrenaline component.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct InnateSkillBreakpoint {
    /// HP fraction at which this breakpoint applies (1.0 = full health).
    health_fraction: f32,
    modifiers: Vec<CrewSkillModifier>,
}

impl InnateSkillBreakpoint {
    pub(super) fn new(health_fraction: f32, modifiers: Vec<CrewSkillModifier>) -> Self {
        Self { health_fraction, modifiers }
    }

    pub fn health_fraction(&self) -> f32 {
        self.health_fraction
    }

    pub fn modifiers(&self) -> &[CrewSkillModifier] {
        &self.modifiers
    }
}

/// A ship innate skill (HP-breakpoint adrenaline) from the hull's `innateSkills` component.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct InnateSkill {
    skill_type: String,
    breakpoints: Vec<InnateSkillBreakpoint>,
}

impl InnateSkill {
    pub(super) fn new(skill_type: String, breakpoints: Vec<InnateSkillBreakpoint>) -> Self {
        Self { skill_type, breakpoints }
    }

    pub fn skill_type(&self) -> &str {
        &self.skill_type
    }

    pub fn breakpoints(&self) -> &[InnateSkillBreakpoint] {
        &self.breakpoints
    }
}

#[derive(Clone, Builder, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct CrewSkillTiers {
    aircraft_carrier: SkillPointCost,
    auxiliary: SkillPointCost,
    battleship: SkillPointCost,
    cruiser: SkillPointCost,
    destroyer: SkillPointCost,
    submarine: SkillPointCost,
}

impl CrewSkillTiers {
    pub fn get_for_species(&self, species: Species) -> SkillPointCost {
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

    pub fn aircraft_carrier(&self) -> SkillPointCost {
        self.aircraft_carrier
    }

    pub fn auxiliary(&self) -> SkillPointCost {
        self.auxiliary
    }

    pub fn battleship(&self) -> SkillPointCost {
        self.battleship
    }

    pub fn cruiser(&self) -> SkillPointCost {
        self.cruiser
    }

    pub fn destroyer(&self) -> SkillPointCost {
        self.destroyer
    }

    pub fn submarine(&self) -> SkillPointCost {
        self.submarine
    }
}

#[derive(Clone, Builder, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct CrewSkill {
    internal_name: CrewSkillName,
    logic_trigger: Option<CrewSkillLogicTrigger>,
    can_be_learned: bool,
    is_epic: bool,
    modifiers: Option<Vec<CrewSkillModifier>>,
    skill_type: CrewSkillType,
    tier: CrewSkillTiers,
    ui_treat_as_trigger: bool,
}

impl CrewSkill {
    pub fn internal_name(&self) -> &CrewSkillName {
        &self.internal_name
    }

    /// Build the gettext id for a skill string, choosing the key style by game
    /// version. The captain-skill rework ([`CAPTAIN_SKILL_REWORK_VERSION`])
    /// changed both the skill data shape and the translation keys: pre-rework
    /// skills key as `<prefix>_<UPPERCASE>` (the internal name is the modifier,
    /// e.g. `PriorityTargetModifier` -> `IDS_SKILL_PRIORITYTARGETMODIFIER`),
    /// while the rework switched to `<prefix>_<UPPER_SNAKE>`. The non-matching
    /// style is returned as a fallback so a stray key from either era still
    /// resolves.
    fn skill_translation_keys(&self, prefix: &str, version: &Version) -> (String, String) {
        use convert_case::Case;
        use convert_case::Casing;
        let snake = format!("{prefix}_{}", self.internal_name().as_str().to_case(Case::UpperSnake));
        let plain = format!("{prefix}_{}", self.internal_name().as_str().to_uppercase());
        let rework = Version::base(
            CAPTAIN_SKILL_REWORK_VERSION.0,
            CAPTAIN_SKILL_REWORK_VERSION.1,
            CAPTAIN_SKILL_REWORK_VERSION.2,
        );
        if version.is_at_least(&rework) { (snake, plain) } else { (plain, snake) }
    }

    pub fn translated_name(&self, metadata_provider: &GameMetadataProvider, version: &Version) -> Option<String> {
        let (primary, fallback) = self.skill_translation_keys("IDS_SKILL", version);
        metadata_provider
            .localized_name_from_id(&TranslationKey::new(primary))
            .or_else(|| metadata_provider.localized_name_from_id(&TranslationKey::new(fallback)))
    }

    /// Expose `skill_translation_keys` to the describe module via the trait
    /// object boundary (it builds keys without the concrete provider).
    pub(crate) fn skill_translation_keys_pub(&self, prefix: &str, version: &Version) -> (String, String) {
        self.skill_translation_keys(prefix, version)
    }

    /// Expose `description_with` (static-or-generated) to the describe module
    /// via `&dyn ResourceLoader`.
    pub(crate) fn description_with_pub(
        &self,
        species: Species,
        metadata: &dyn crate::data::ResourceLoader,
        version: &Version,
    ) -> Option<String> {
        self.description_with(species, metadata, version)
    }

    pub fn translated_description(
        &self,
        metadata_provider: &GameMetadataProvider,
        version: &Version,
    ) -> Option<String> {
        self.translated_description_with(metadata_provider, version)
    }

    fn translated_description_with(
        &self,
        metadata: &dyn crate::data::ResourceLoader,
        version: &Version,
    ) -> Option<String> {
        let (primary, fallback) = self.skill_translation_keys("IDS_SKILL_DESC", version);
        let description = metadata
            .localized_name_from_id(&TranslationKey::new(primary))
            .or_else(|| metadata.localized_name_from_id(&TranslationKey::new(fallback)));

        description.and_then(|desc| if desc.is_empty() || desc == " " { None } else { Some(desc) })
    }

    /// Static localized description when present, else a description generated
    /// from this skill's modifiers (and its logic-trigger modifiers, labeled as
    /// triggered effects). Returns `None` when neither source yields any text.
    pub fn description(&self, species: Species, metadata: &GameMetadataProvider, version: &Version) -> Option<String> {
        self.description_with(species, metadata, version)
    }

    fn description_with(
        &self,
        species: Species,
        metadata: &dyn crate::data::ResourceLoader,
        version: &Version,
    ) -> Option<String> {
        if let Some(d) = self.translated_description_with(metadata, version) {
            return Some(d);
        }
        let mut lines: Vec<String> = Vec::new();
        if let Some(mods) = self.modifiers() {
            lines.extend(crate::game_params::modifier_settings_data::describe_modifiers(
                *version,
                mods.iter().map(|m| (m.name(), m.get_for_species(&species))),
                species,
                metadata,
            ));
        }
        if let Some(trig) = self.logic_trigger() {
            // The trigger condition sentence is keyed by trigger TYPE (e.g.
            // activationOnDetectTrigger -> IDS_SKILL_TRIGGER_ACTIVATIONONDETECTTRIGGER);
            // triggerDescIds is often empty.
            let sentence = metadata
                .localized_name_from_id(&TranslationKey::new(format!(
                    "IDS_SKILL_TRIGGER_{}",
                    trig.trigger_type().to_uppercase()
                )))
                .and_then(|s| if s.is_empty() || s == " " { None } else { Some(s) });
            let has_sentence = sentence.is_some();
            if let Some(sentence) = sentence {
                lines.push(sentence);
            }
            if let Some(tmods) = trig.modifiers() {
                for line in crate::game_params::modifier_settings_data::describe_modifiers(
                    *version,
                    tmods.iter().map(|m| (m.name(), m.get_for_species(&species))),
                    species,
                    metadata,
                ) {
                    // The sentence already conveys the condition; only fall back
                    // to the suffix when no sentence resolved.
                    if has_sentence {
                        lines.push(line);
                    } else {
                        lines.push(format!("{line} (when triggered)"));
                    }
                }
            }
        }
        (!lines.is_empty()).then(|| lines.join("\n"))
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

    pub fn skill_type(&self) -> CrewSkillType {
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
    pub fn skill_by_type(&self, typ: CrewSkillType) -> Option<&CrewSkill> {
        self.skills.as_ref().and_then(|skills| skills.iter().find(|skill| skill.skill_type == typ))
    }

    pub fn skills(&self) -> Option<&[CrewSkill]> {
        self.skills.as_deref()
    }

    pub fn personality(&self) -> &CrewPersonality {
        &self.personality
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

    /// Determine the effective plane category for display/icon purposes.
    ///
    /// The raw `category` from `planeSubtype` isn't always sufficient:
    /// - Airship/Auxiliary species are event airships that always use controllable-style icons,
    ///   even though their `planeSubtype` is empty (which defaults to `Controllable`).
    /// - Controllable CV planes always have an ammo type; if ammo is empty and the plane isn't
    ///   an airship, it's actually a consumable (catapult fighter/spotter).
    pub fn effective_category(&self, species: Option<&Species>) -> PlaneCategory {
        let is_airship = matches!(species, Some(Species::Airship | Species::Auxiliary));
        if is_airship {
            return PlaneCategory::Controllable;
        }
        if matches!(self.category, PlaneCategory::Controllable) && self.ammo_type.is_empty() {
            return PlaneCategory::Consumable;
        }
        self.category.clone()
    }
}

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
    /// Fuse arming threshold in mm. `None` when the projectile carries no fuse
    /// data; there is no safe numeric default (0.0 would arm on any plate).
    pub fuse_threshold: Option<f32>,
    pub burn_prob: f32,
    pub air_drag: f32,
    /// Cap normalization angle in degrees. `None` when the projectile defines no
    /// normalization; there is no safe numeric default (0.0 changes penetration).
    pub normalization: Option<f32>,
    /// Whether the shell is capped (`bulletCap`). Capped shells benefit from
    /// normalization; uncapped shells do not. Defaults to the game's `Ammo`
    /// base-class value (`true`) when the field is absent.
    pub cap: bool,
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
    /// Flood chance (`uwCritical`). Read by `PreprocessedAmmo.floodChance`
    /// (PreprocessedAmmo.py:19) and surfaced as the HE shell's `floodChance`
    /// (FactoryArtillery.py:172). 0.0 on shells that cannot flood.
    #[cfg_attr(feature = "serde", serde(default))]
    uw_critical: Option<f32>,
    /// Shell speed multiplier (`timeFactor`). `speed = bulletSpeed * timeFactor`
    /// (PreprocessedAmmo.py:16); `maa3520d6.py:1151` defaults it to 1.0 when absent.
    #[cfg_attr(feature = "serde", serde(default))]
    time_factor: Option<f32>,
    /// Air drag coefficient.
    #[cfg_attr(feature = "serde", serde(default))]
    bullet_air_drag: Option<f32>,
    /// Torpedo travel speed in knots.
    #[cfg_attr(feature = "serde", serde(default))]
    speed: Option<f32>,
    /// Secondary (flood) damage component, distinct from `alpha_damage`.
    #[cfg_attr(feature = "serde", serde(default))]
    damage: Option<f32>,
    /// Detectability factor (km-scaled visibility coefficient).
    #[cfg_attr(feature = "serde", serde(default))]
    visibility_factor: Option<f32>,
    /// Distance-keyed damage falloff pairs `(coeff, distBW)`. Empty on most torpedoes.
    #[cfg_attr(feature = "serde", serde(default))]
    distance_of_damage: Option<Vec<(f32, f32)>>,
    /// Torpedo type discriminant (e.g. 0 = normal, 1 = deep-water).
    #[cfg_attr(feature = "serde", serde(default))]
    torpedo_type: Option<i64>,
    /// Ship classes this torpedo passes under (deep-water torpedoes).
    #[cfg_attr(feature = "serde", serde(default))]
    ignore_classes: Option<Vec<String>>,
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

    /// Flood chance (`uwCritical`), the HE shell's `floodChance`
    /// (PreprocessedAmmo.py:19, FactoryArtillery.py:172).
    pub fn uw_critical(&self) -> Option<f32> {
        self.uw_critical
    }

    /// Shell speed multiplier (`timeFactor`), `speed = bulletSpeed * timeFactor`
    /// (PreprocessedAmmo.py:16). `None` when absent; the GameParams class default is 1.0.
    pub fn time_factor(&self) -> Option<f32> {
        self.time_factor
    }

    pub fn bullet_air_drag(&self) -> Option<f32> {
        self.bullet_air_drag
    }

    pub fn speed(&self) -> Option<f32> {
        self.speed
    }

    pub fn damage(&self) -> Option<f32> {
        self.damage
    }

    pub fn visibility_factor(&self) -> Option<f32> {
        self.visibility_factor
    }

    pub fn distance_of_damage(&self) -> Option<&[(f32, f32)]> {
        self.distance_of_damage.as_deref()
    }

    pub fn torpedo_type(&self) -> Option<i64> {
        self.torpedo_type
    }

    pub fn ignore_classes(&self) -> Option<&[String]> {
        self.ignore_classes.as_deref()
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
            fuse_threshold: self.bullet_detonator_threshold,
            burn_prob: self.burn_prob.unwrap_or(-0.5),
            air_drag: self.bullet_air_drag.unwrap_or(0.0),
            normalization: self.bullet_cap_normalize_max_angle,
            cap: self.bullet_cap.unwrap_or(true),
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
    /// Upgrade slot index (0-based). `None` when the game slot is `< 0` (not slotted).
    slot: Option<u8>,
    ship_levels: Vec<u32>,
    ship_types: Vec<String>,
    nations: Vec<String>,
    groups: Vec<String>,
    ships: Vec<String>,
    excludes: Vec<String>,
}

impl Modernization {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        modifiers: Vec<CrewSkillModifier>,
        slot: Option<u8>,
        ship_levels: Vec<u32>,
        ship_types: Vec<String>,
        nations: Vec<String>,
        groups: Vec<String>,
        ships: Vec<String>,
        excludes: Vec<String>,
    ) -> Self {
        Self { modifiers, slot, ship_levels, ship_types, nations, groups, ships, excludes }
    }

    pub fn modifiers(&self) -> &[CrewSkillModifier] {
        &self.modifiers
    }

    pub fn slot(&self) -> Option<u8> {
        self.slot
    }

    /// Port of the client's `isModApplicable`. An empty criterion list (e.g. empty
    /// `ship_types`) is NOT "matches all": membership is false, so the mod falls
    /// through to `name in ships` -- matching the game (validated: Montana = 6 slots).
    pub fn applies_to(
        &self,
        ship_name: &str,
        ship_level: u32,
        ship_species: &str,
        ship_nation: &str,
        ship_group: &str,
    ) -> bool {
        if self.excludes.iter().any(|s| s == ship_name) {
            return false;
        }
        let in_ships = self.ships.iter().any(|s| s == ship_name);
        if !self.groups.iter().any(|g| g == ship_group) {
            return in_ships;
        }
        if !self.nations.iter().any(|n| n == ship_nation) {
            return in_ships;
        }
        if !self.ship_types.iter().any(|t| t == ship_species) {
            return in_ships;
        }
        self.ship_levels.contains(&ship_level) || in_ships
    }
}

/// A ship module/component param (GameParams `Unit` type), e.g. a hull, main
/// battery, torpedo, or fire-control module. Carries the raw `ucType` string the
/// param reports, which names the concrete component slot the module occupies.
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct Unit {
    /// Raw `ucType` string from GameParams (e.g. `"_Hull"`, `"_Torpedoes"`,
    /// `"_Suo"`). `None` when the param carries no (or an empty) `ucType`.
    uc_type: Option<String>,
}

impl Unit {
    pub fn new(uc_type: Option<String>) -> Self {
        Self { uc_type }
    }

    /// Raw `ucType` string from GameParams (e.g. `"_Hull"`, `"_Torpedoes"`,
    /// `"_Suo"`). `None` when the unit has no `ucType` field.
    pub fn uc_type(&self) -> Option<&str> {
        self.uc_type.as_deref()
    }
}

/// Number of modernization (upgrade) slots a ship has: max applicable `slot` + 1.
/// Returns 0 when `ship` is not a vehicle or no slotted modernization applies.
pub fn modernization_slot_count(params: &[crate::Rc<Param>], ship: &Param) -> usize {
    let Some(vehicle) = ship.vehicle() else { return 0 };
    let Some(species) = ship.species().and_then(|r| r.known()) else { return 0 };
    let species_name = species.name();
    let ship_name = ship.name();
    let level = vehicle.level();
    let group = vehicle.group();
    let nation = ship.nation();
    let mut max_slot: i32 = -1;
    for p in params {
        let Some(m) = p.modernization() else { continue };
        let Some(slot) = m.slot() else { continue };
        if m.applies_to(ship_name, level, species_name, nation, group) {
            max_slot = max_slot.max(slot as i32);
        }
    }
    (max_slot + 1) as usize
}

#[cfg(test)]
mod modernization_tests {
    use super::Modernization;

    fn mk(
        slot: Option<u8>,
        levels: &[u32],
        types: &[&str],
        nations: &[&str],
        groups: &[&str],
        ships: &[&str],
        excludes: &[&str],
    ) -> Modernization {
        Modernization::new(
            Vec::new(),
            slot,
            levels.to_vec(),
            types.iter().map(|s| s.to_string()).collect(),
            nations.iter().map(|s| s.to_string()).collect(),
            groups.iter().map(|s| s.to_string()).collect(),
            ships.iter().map(|s| s.to_string()).collect(),
            excludes.iter().map(|s| s.to_string()).collect(),
        )
    }

    #[test]
    fn applies_respects_excludes_and_chain() {
        let m = mk(
            Some(2),
            &[8, 9, 10],
            &["Battleship"],
            &["USA"],
            &["upgradeable"],
            &["PASB013_Arkansas_1912"],
            &["PASB099_Excluded"],
        );
        // excluded by name
        assert!(!m.applies_to("PASB099_Excluded", 10, "Battleship", "USA", "upgradeable"));
        // full match + level in shiplevel
        assert!(m.applies_to("PASB017_Montana_1945", 10, "Battleship", "USA", "upgradeable"));
        // wrong type, not in ships -> false
        assert!(!m.applies_to("PASC001_Foo", 10, "Cruiser", "USA", "upgradeable"));
        // wrong type but explicitly in ships -> true
        assert!(m.applies_to("PASB013_Arkansas_1912", 3, "Cruiser", "USA", "upgradeable"));
        // full match but level not in shiplevel and not in ships -> false
        assert!(!m.applies_to("PASB500_LowTier", 5, "Battleship", "USA", "upgradeable"));
    }

    #[test]
    fn empty_lists_fall_through_to_ships_not_match_all() {
        // Empty criteria are not "match all"; a mod with explicit ships applies only to them.
        let m = mk(Some(0), &[], &[], &[], &[], &["PASB013_Arkansas_1912"], &[]);
        assert!(m.applies_to("PASB013_Arkansas_1912", 3, "Battleship", "USA", "upgradeable"));
        assert!(!m.applies_to("PASB017_Montana_1945", 10, "Battleship", "USA", "upgradeable"));
        // All-empty (including ships) applies to nobody.
        let none = mk(Some(0), &[], &[], &[], &[], &[], &[]);
        assert!(!none.applies_to("PASB017_Montana_1945", 10, "Battleship", "USA", "upgradeable"));
    }
}

/// A building/structure game parameter (Operations forts, AA batteries, airbases, etc.).
///
/// The species (AntiAircraft, Complex, AirBase, CoastalArtillery, etc.) is stored
/// in the parent [`Param`]'s `species` field.
#[derive(Builder, Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct Building {
    level: u32,
    health: f32,
}

impl Building {
    pub fn level(&self) -> u32 {
        self.level
    }

    pub fn health(&self) -> f32 {
        self.health
    }
}

// Boxing the larger variants would change the rkyv archived layout (forcing a
// FORMAT_VERSION bump and a full re-derive) and add heap indirection to every
// param access; the size spread is inherent to the domain model.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub enum ParamData {
    Vehicle(Vehicle),
    Crew(Crew),
    Ability(Ability),
    Achievement(Achievement),
    Modernization(Modernization),
    Exterior(Exterior),
    Unit(Unit),
    Aircraft(Aircraft),
    Projectile(Projectile),
    Drop(BuffDrop),
    Building(Building),
}

variant_accessors!(ParamData {
    tuple Vehicle(Vehicle) => vehicle;
    tuple Crew(Crew) => crew;
    tuple Ability(Ability) => ability;
    tuple Achievement(Achievement) => achievement;
    tuple Modernization(Modernization) => modernization;
    tuple Exterior(Exterior) => exterior;
    tuple Aircraft(Aircraft) => aircraft;
    tuple Projectile(Projectile) => projectile;
    tuple Drop(BuffDrop) => drop;
    tuple Building(Building) => building;
    tuple Unit(Unit) => unit;
});

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

/// Resolve a ship's representative secondary (ATBA) ammo projectile GameParamId.
///
/// Ships with mixed-caliber secondaries (e.g. German BBs with 105mm and 150mm
/// mounts) collect more than one ammo name. Per-gun resolution would need the
/// client's gun ordering, which comes from the model/visual hardpoint layout and
/// is out of scope, so one representative is chosen. The choice is the
/// lexicographically smallest name purely to be deterministic across runs (the
/// set is otherwise unordered). The ammo type is uniform per ship in practice,
/// so the dot color is correct; only the muzzle-speed pacing is approximate for
/// mixed-caliber ships. Returns None for ships without secondaries or on builds
/// predating the data.
pub fn secondary_ammo_param<P: GameParamProvider + ?Sized>(provider: &P, ship: GameParamId) -> Option<GameParamId> {
    let ship_param = provider.game_param_by_id(ship)?;
    let ammo_name = ship_param.vehicle()?.config_data()?.secondary_battery_ammo.iter().min()?;
    Some(provider.game_param_by_name(ammo_name)?.id())
}

/// Whether `ammo` is one of `ship`'s secondary (ATBA) battery shells.
///
/// Secondary fire arrives through the same `receiveArtilleryShots` path as the
/// main battery, carrying the secondary shell's GameParamId; the packet has no
/// weapon-type flag and the projectile species is `Artillery` for both. The only
/// discriminator is whether the shell belongs to the ship's ATBA ammo set rather
/// than its main battery, so the renderer can dim secondary tracers.
pub fn is_secondary_ammo<P: GameParamProvider + ?Sized>(provider: &P, ship: GameParamId, ammo: GameParamId) -> bool {
    let Some(ship_param) = provider.game_param_by_id(ship) else {
        return false;
    };
    let Some(config) = ship_param.vehicle().and_then(|v| v.config_data()) else {
        return false;
    };
    let Some(ammo_param) = provider.game_param_by_id(ammo) else {
        return false;
    };
    config.secondary_battery_ammo.contains(ammo_param.name())
}

/// Resolve the secondary ammo projectile GameParamId for a specific gun.
///
/// `gun` is the wire gunID; it indexes `secondary_guns`, which is ordered by
/// hardpoint number under the assumption that the client's gun list follows that
/// order (the exact order would otherwise need model/visual data). Falls back to
/// the representative `secondary_ammo_param` when the gun index is out of range
/// or its ammo name does not resolve.
pub fn secondary_gun_ammo_param<P: GameParamProvider + ?Sized>(
    provider: &P,
    ship: GameParamId,
    gun: GunId,
) -> Option<GameParamId> {
    let ship_param = provider.game_param_by_id(ship)?;
    let guns = &ship_param.vehicle()?.config_data()?.secondary_guns;
    let name = guns.get(gun.index()).filter(|n| !n.is_empty());
    match name.and_then(|n| provider.game_param_by_name(n)) {
        Some(p) => Some(p.id()),
        None => secondary_ammo_param(provider, ship),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secondary_ammo_strings_map_to_ammo_type() {
        // Secondaries are HE or SAP ("CS" in game data); both must resolve so the
        // renderer can color the dot.
        assert_eq!(AmmoType::from_game_str("HE"), AmmoType::HE);
        assert_eq!(AmmoType::from_game_str("CS"), AmmoType::SAP);
    }

    /// Worcester HE shell `PAPA051_152mm_HE_HC_Mark_39_Mod_0` (GameParams.json, build
    /// 14.6 stable): the fields `createAmmoTTX`/`PreprocessedAmmo` read off a shell
    /// (FactoryArtillery.py:147-190, PreprocessedAmmo.py:11-21) round-trip through the
    /// parsed `Projectile`. `uw_critical` is the newly added flood-chance field.
    #[test]
    fn worcester_he_shell_fields() {
        let p = Projectile::builder()
            .ammo_type("HE".to_string())
            .alpha_damage(2200.0)
            .alpha_piercing_he(30.0)
            .burn_prob(0.12)
            .uw_critical(0.0)
            .bullet_diametr(0.152)
            .bullet_speed(812.0)
            .bullet_mass(47.6)
            .bullet_krupp(1150.0)
            .build();
        assert_eq!(p.ammo_type(), "HE");
        assert_eq!(p.alpha_damage(), Some(2200.0));
        assert_eq!(p.alpha_piercing_he(), Some(30.0));
        assert_eq!(p.burn_prob(), Some(0.12));
        assert_eq!(p.uw_critical(), Some(0.0));
        assert_eq!(p.bullet_diametr(), Some(0.152));
        assert_eq!(p.bullet_speed(), Some(812.0));
    }

    /// Worcester AP shell `PAPA050_152mm_AP_130lbs_Mk35` (GameParams.json): AP carries
    /// `burnProb -0.5` (the "N/A" sentinel) and `uwCritical 0.0`; the AP-relevant
    /// ballistic fields (mass/krupp/speed/caliber) are present for a later sim.
    #[test]
    fn worcester_ap_shell_fields() {
        let p = Projectile::builder()
            .ammo_type("AP".to_string())
            .alpha_damage(3200.0)
            .alpha_piercing_he(0.0)
            .burn_prob(-0.5)
            .uw_critical(0.0)
            .bullet_diametr(0.152)
            .bullet_speed(762.0)
            .bullet_mass(59.0)
            .bullet_krupp(2692.0)
            .build();
        assert_eq!(p.ammo_type(), "AP");
        assert_eq!(p.alpha_damage(), Some(3200.0));
        assert_eq!(p.burn_prob(), Some(-0.5));
        assert_eq!(p.uw_critical(), Some(0.0));
        assert_eq!(p.bullet_mass(), Some(59.0));
        assert_eq!(p.bullet_krupp(), Some(2692.0));
        assert_eq!(p.bullet_speed(), Some(762.0));
    }

    fn shell_param(id: u32, name: &str) -> Param {
        Param {
            id: GameParamId::from(id),
            index: name.to_string(),
            name: name.to_string(),
            species: None,
            nation: String::new(),
            data: ParamData::Unit(Unit::new(None)),
        }
    }

    #[test]
    fn is_secondary_ammo_matches_only_the_atba_set() {
        let ship_id = GameParamId::from(100u32);
        let secondary_id = GameParamId::from(200u32);
        let main_id = GameParamId::from(300u32);

        let config = ShipConfigData {
            secondary_battery_ammo: HashSet::from(["SECONDARY_SHELL".to_string()]),
            ..Default::default()
        };
        let vehicle = Vehicle {
            level: 0,
            group: String::new(),
            abilities: None,
            upgrades: Vec::new(),
            config_data: Some(config),
            model_path: None,
            armor: None,
            hit_locations: None,
            permoflages: Vec::new(),
            camera_trajectories: Vec::new(),
            ttx_components: None,
            innate_skills: Vec::new(),
        };
        let ship = Param {
            id: ship_id,
            index: "ship".to_string(),
            name: "SHIP".to_string(),
            species: None,
            nation: String::new(),
            data: ParamData::Vehicle(vehicle),
        };

        let params = GameParams::from(vec![ship, shell_param(200, "SECONDARY_SHELL"), shell_param(300, "MAIN_SHELL")]);

        assert!(is_secondary_ammo(&params, ship_id, secondary_id), "ATBA shell must classify as secondary");
        assert!(!is_secondary_ammo(&params, ship_id, main_id), "main battery shell must not classify as secondary");
        // Unknown ship or unknown ammo resolves to false rather than panicking.
        assert!(!is_secondary_ammo(&params, GameParamId::from(999u32), secondary_id));
        assert!(!is_secondary_ammo(&params, ship_id, GameParamId::from(999u32)));
    }
}

#[cfg(test)]
mod skill_type_tests {
    use super::CrewSkillType;

    #[test]
    fn crew_skill_type_roundtrips_raw() {
        let t = CrewSkillType::new(67);
        assert_eq!(t.raw(), 67);
        assert_eq!(CrewSkillType::from(67u8), CrewSkillType::new(67));
    }
}

#[cfg(test)]
mod point_cost_tests {
    use super::SkillPointCost;

    #[test]
    fn from_grid_row_is_one_based() {
        assert_eq!(SkillPointCost::from_grid_row(0).get(), 1);
        assert_eq!(SkillPointCost::from_grid_row(3).get(), 4);
    }
}

#[cfg(test)]
mod skill_name_tests {
    use super::CrewSkillName;

    #[test]
    fn as_str_and_display_match() {
        let n = CrewSkillName::from("TriggerSpreading");
        assert_eq!(n.as_str(), "TriggerSpreading");
        assert_eq!(n.to_string(), "TriggerSpreading");
    }
}

#[cfg(test)]
mod crew_skill_description_tests {
    use super::*;
    use crate::data::Version;

    /// Returns any requested id verbatim, standing in for present static text.
    struct EchoLoader;
    impl crate::data::ResourceLoader for EchoLoader {
        fn localized_name_from_param(&self, _param: &Param) -> Option<String> {
            None
        }
        fn localized_name_from_id(&self, id: &crate::data::TranslationKey) -> Option<String> {
            Some(id.as_str().to_string())
        }
        fn game_param_by_id(&self, _id: crate::game_types::GameParamId) -> Option<crate::Rc<Param>> {
            None
        }
        fn entity_specs(&self) -> &[crate::rpc::entitydefs::EntitySpec] {
            &[]
        }
    }

    /// Resolves only modifier label/unit ids, leaving skill description ids
    /// and trigger-type sentence ids empty so the generated path runs and the
    /// "(when triggered)" suffix fallback is exercised.
    struct ModifierOnlyLoader;
    impl crate::data::ResourceLoader for ModifierOnlyLoader {
        fn localized_name_from_param(&self, _param: &Param) -> Option<String> {
            None
        }
        fn localized_name_from_id(&self, id: &crate::data::TranslationKey) -> Option<String> {
            if id.as_str().starts_with("IDS_SKILL_DESC") || id.as_str().starts_with("IDS_SKILL_TRIGGER_") {
                None
            } else {
                Some(id.as_str().to_string())
            }
        }
        fn game_param_by_id(&self, _id: crate::game_types::GameParamId) -> Option<crate::Rc<Param>> {
            None
        }
        fn entity_specs(&self) -> &[crate::rpc::entitydefs::EntitySpec] {
            &[]
        }
    }

    /// Like [`ModifierOnlyLoader`] but resolves trigger-type sentence ids,
    /// standing in for a catalog that has the trigger condition text.
    struct TriggerSentenceLoader;
    impl crate::data::ResourceLoader for TriggerSentenceLoader {
        fn localized_name_from_param(&self, _param: &Param) -> Option<String> {
            None
        }
        fn localized_name_from_id(&self, id: &crate::data::TranslationKey) -> Option<String> {
            if id.as_str().starts_with("IDS_SKILL_DESC") { None } else { Some(id.as_str().to_string()) }
        }
        fn game_param_by_id(&self, _id: crate::game_types::GameParamId) -> Option<crate::Rc<Param>> {
            None
        }
        fn entity_specs(&self) -> &[crate::rpc::entitydefs::EntitySpec] {
            &[]
        }
    }

    fn version() -> Version {
        Version::base(15, 0, 0)
    }

    fn tiers() -> CrewSkillTiers {
        CrewSkillTiers::builder()
            .aircraft_carrier(SkillPointCost::new(1))
            .auxiliary(SkillPointCost::new(1))
            .battleship(SkillPointCost::new(1))
            .cruiser(SkillPointCost::new(1))
            .destroyer(SkillPointCost::new(1))
            .submarine(SkillPointCost::new(1))
            .build()
    }

    fn uniform_modifier(name: &str, value: f32) -> CrewSkillModifier {
        CrewSkillModifier::builder()
            .name(name.to_owned())
            .aircraft_carrier(value)
            .auxiliary(value)
            .battleship(value)
            .cruiser(value)
            .destroyer(value)
            .submarine(value)
            .excluded_consumables(Vec::new())
            .build()
    }

    fn skill_with_modifiers(modifiers: Vec<CrewSkillModifier>) -> CrewSkill {
        CrewSkill::builder()
            .internal_name(CrewSkillName::from("GunFeeder"))
            .can_be_learned(true)
            .is_epic(false)
            .skill_type(CrewSkillType::new(1))
            .ui_treat_as_trigger(false)
            .tier(tiers())
            .modifiers(modifiers)
            .build()
    }

    #[test]
    fn static_description_wins_over_generated() {
        // With a loader that resolves the skill desc id, description returns that
        // static text and never falls through to modifier generation.
        let skill = skill_with_modifiers(vec![uniform_modifier("GMRotationSpeed", 0.9)]);
        let out =
            skill.description_with(Species::Battleship, &EchoLoader, &version()).expect("static description present");
        assert!(out.contains("IDS_SKILL_DESC"), "expected static desc id, got {out}");
        assert!(!out.contains("IDS_PARAMS_MODIFIER"), "should not generate, got {out}");
    }

    #[test]
    fn generates_from_modifiers_when_static_absent() {
        let skill = skill_with_modifiers(vec![uniform_modifier("GMRotationSpeed", 0.9)]);
        let out = skill
            .description_with(Species::Battleship, &ModifierOnlyLoader, &version())
            .expect("modifier-generated description");
        assert!(out.contains("IDS_PARAMS_MODIFIER_GMROTATIONSPEED"), "got {out}");
        assert!(!out.contains("(when triggered)"), "plain modifier, got {out}");
    }

    fn trigger_with(trigger_type: &str, modifiers: Vec<CrewSkillModifier>) -> CrewSkillLogicTrigger {
        CrewSkillLogicTrigger::builder()
            .consumable_type(String::new())
            .cooling_delay(0.0)
            .cooling_interpolator(Interpolator::default())
            .count_to_modifier(Vec::new())
            .duration(0.0)
            .energy_coeff(0.0)
            .heat_interpolator(Interpolator::default())
            .modifiers(modifiers)
            .trigger_desc_ids(String::new())
            .trigger_type(trigger_type.to_owned())
            .build()
    }

    fn skill_with_trigger(trigger: CrewSkillLogicTrigger) -> CrewSkill {
        CrewSkill::builder()
            .internal_name(CrewSkillName::from("GunFeeder"))
            .can_be_learned(true)
            .is_epic(false)
            .skill_type(CrewSkillType::new(1))
            .ui_treat_as_trigger(false)
            .tier(tiers())
            .logic_trigger(trigger)
            .build()
    }

    #[test]
    fn trigger_modifiers_are_labeled_when_triggered_without_sentence() {
        // No trigger-type sentence resolves, so the modifier line keeps the
        // "(when triggered)" suffix to still convey the condition.
        let trigger = trigger_with("activationOnDetectTrigger", vec![uniform_modifier("GMRotationSpeed", 1.1)]);
        let skill = skill_with_trigger(trigger);
        let out = skill
            .description_with(Species::Battleship, &ModifierOnlyLoader, &version())
            .expect("trigger-generated description");
        assert!(out.contains("(when triggered)"), "got {out}");
        assert!(out.contains("IDS_PARAMS_MODIFIER_GMROTATIONSPEED"), "got {out}");
    }

    #[test]
    fn trigger_sentence_precedes_unsuffixed_modifier_lines() {
        // When the trigger-type sentence resolves, it leads and the trigger
        // modifier lines drop the "(when triggered)" suffix.
        let trigger = trigger_with("activationOnDetectTrigger", vec![uniform_modifier("GMRotationSpeed", 1.1)]);
        let skill = skill_with_trigger(trigger);
        let out = skill
            .description_with(Species::Battleship, &TriggerSentenceLoader, &version())
            .expect("trigger-generated description");
        let mut iter = out.lines();
        assert_eq!(iter.next(), Some("IDS_SKILL_TRIGGER_ACTIVATIONONDETECTTRIGGER"), "sentence should lead, got {out}");
        assert!(out.contains("IDS_PARAMS_MODIFIER_GMROTATIONSPEED"), "got {out}");
        assert!(!out.contains("(when triggered)"), "suffix should be dropped, got {out}");
    }

    #[test]
    fn no_text_yields_none() {
        // No static desc and no modifiers means nothing to show.
        let skill = skill_with_modifiers(Vec::new());
        assert!(skill.description_with(Species::Battleship, &ModifierOnlyLoader, &version()).is_none());
    }

    #[test]
    fn logic_trigger_consumable_type_and_duration() {
        let trigger = CrewSkillLogicTrigger::builder()
            .consumable_type("hydrophone".to_owned())
            .cooling_delay(0.0)
            .cooling_interpolator(Interpolator::default())
            .count_to_modifier(Vec::new())
            .duration(15.0)
            .energy_coeff(0.0)
            .heat_interpolator(Interpolator::default())
            .trigger_desc_ids(String::new())
            .trigger_type("activationOnConsumable".to_owned())
            .build();
        let v = crate::data::Version::base(15, 4, 0);
        assert_eq!(trigger.consumable_type(v).into_known(), Some(crate::game_types::Consumable::Hydrophone));
        assert_eq!(trigger.duration(), 15.0);
    }
}

#[cfg(test)]
mod known_skill_tests {
    use super::CrewSkillName;
    use super::CrewSkillType;
    use super::KnownCrewSkill;
    use crate::recognized::Recognized;

    #[test]
    fn recognizes_known_skills_by_name() {
        let dazzle = KnownCrewSkill::recognize(&CrewSkillName::from("TriggerSpreading"), CrewSkillType::new(67));
        assert_eq!(dazzle, Recognized::Known(KnownCrewSkill::Dazzle));

        let ifhe = KnownCrewSkill::recognize(&CrewSkillName::from("HePenetration"), CrewSkillType::new(33));
        assert_eq!(ifhe, Recognized::Known(KnownCrewSkill::InertiaFuse));

        let concealment =
            KnownCrewSkill::recognize(&CrewSkillName::from("DetectionVisibilityRange"), CrewSkillType::new(14));
        assert_eq!(concealment, Recognized::Known(KnownCrewSkill::ConcealmentExpert));

        let sub_adrenaline =
            KnownCrewSkill::recognize(&CrewSkillName::from("ArmamentReloadSubmarine"), CrewSkillType::new(82));
        assert_eq!(sub_adrenaline, Recognized::Known(KnownCrewSkill::SubmarineAdrenalineRush));
    }

    #[test]
    fn detection_alert_is_incoming_fire_alert() {
        // "IFA" siren marker means Incoming Fire Alert (DetectionAlert), not Inertia Fuse.
        let ifa = KnownCrewSkill::recognize(&CrewSkillName::from("DetectionAlert"), CrewSkillType::new(19));
        assert_eq!(ifa, Recognized::Known(KnownCrewSkill::IncomingFireAlert));
    }

    #[test]
    fn unknown_skill_preserves_raw_type() {
        let other = KnownCrewSkill::recognize(&CrewSkillName::from("NotARealSkill"), CrewSkillType::new(255));
        assert_eq!(other, Recognized::Unknown(CrewSkillType::new(255)));
    }
}

#[cfg(test)]
mod interpolator_tests {
    use super::Interpolator;

    #[test]
    fn empty_by_default() {
        let interp = Interpolator::default();
        assert!(interp.is_empty());
        assert_eq!(interp.points(), &[]);
    }

    #[test]
    fn from_points_round_trips() {
        let pts = vec![(0.0f32, 0.0f32), (10.0, 0.5), (45.0, 1.0)];
        let interp = Interpolator::from_points(pts.clone());
        assert!(!interp.is_empty());
        assert_eq!(interp.points(), pts.as_slice());
    }

    #[test]
    fn single_point_not_empty() {
        let interp = Interpolator::from_points(vec![(1.0, 0.5)]);
        assert!(!interp.is_empty());
        assert_eq!(interp.points().len(), 1);
    }

    #[test]
    fn interpolator_eval_clamped_piecewise_linear() {
        let lerp = Interpolator::from_points(vec![(0.0, 0.0), (10.0, 0.5), (45.0, 1.0)]);
        assert_eq!(lerp.eval(-5.0), 0.0, "below first x clamps to first y");
        assert_eq!(lerp.eval(0.0), 0.0, "first point");
        assert!((lerp.eval(5.0) - 0.25).abs() < 1e-6, "midpoint of [0,0]-[10,0.5]");
        assert!((lerp.eval(10.0) - 0.5).abs() < 1e-6, "second point");
        assert!((lerp.eval(27.5) - 0.75).abs() < 1e-6, "midpoint of [10,0.5]-[45,1.0]");
        assert_eq!(lerp.eval(45.0), 1.0, "last point");
        assert_eq!(lerp.eval(100.0), 1.0, "above last x clamps to last y");
        assert_eq!(lerp.max_x(), 45.0);

        let empty = Interpolator::from_points(Vec::new());
        assert_eq!(empty.eval(5.0), 0.0, "empty -> 0.0");
        assert_eq!(empty.max_x(), 0.0, "empty -> 0.0");
    }
}
