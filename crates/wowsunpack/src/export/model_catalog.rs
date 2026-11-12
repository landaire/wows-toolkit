//! Catalogue of every `.model` prototype in an `assets.bin`.
//!
//! Gameplay models follow `content/gameplay/<nation>/<kind>/[<sub-kind>/]<dir>/<leaf>.model`
//! and are grouped by nation, kind and sub-kind. Models a GameParams `Building`
//! uses (forts, aerodromes, towers) are grouped by building kind. Location and
//! port scenery is grouped by its category directories, and the rest by its
//! top directories.
//!
//! In every directory the model named after the directory is the base model.
//! Its siblings are variants of it: `_dead` wreck models and the hull sections
//! of a ship (`_Bow`, `_MidFront`, `_MidBack`, `_Stern`). Skin models live in
//! their own directories and are linked to their stock model through the
//! GameParams `peculiarityModels` of the exteriors that apply them. A building
//! wreck in its own directory is linked through the building's `deadModel`.

use std::borrow::Borrow;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::HashSet;
use std::fmt;

use thiserror::Error;

use crate::data::TranslationKey;
use crate::data::assets_bin_vfs::PrototypeType;
use crate::game_params::provider::GameMetadataProvider;
use crate::game_params::types::GameParamProvider;
use crate::game_params::types::PeculiarityModel;
use crate::game_params::types::Species;
use crate::models::assets_bin::PrototypeDatabase;
use crate::recognized::Recognized;

use super::part_group::PartGroup;

const GAMEPLAY_PREFIX: &str = "content/gameplay/";
const LOCATION_PREFIX: &str = "content/location/";
const PORTS_PREFIX: &str = "content/ports/";
const STYLES_PREFIX: &str = "content/styles/";
const MODEL_EXTENSION: &str = ".model";
const DEAD_SUFFIX: &str = "_dead";

/// Why a string is not a [`ModelPath`].
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ModelPathError {
    #[error("model path {path:?} does not end in .model")]
    NotAModel { path: String },
    #[error("model path {path:?} has an empty file name")]
    EmptyName { path: String },
}

/// VFS path of a `.model` prototype, without a leading `/`, e.g.
/// `content/gameplay/usa/gun/main/AGM034_16in50_Mk7/AGM034_16in50_Mk7.model`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ModelPath(String);

impl ModelPath {
    pub fn parse(path: &str) -> Result<Self, ModelPathError> {
        let path = path.strip_prefix('/').unwrap_or(path);
        let Some(without_extension) = path.strip_suffix(MODEL_EXTENSION) else {
            return Err(ModelPathError::NotAModel { path: path.to_string() });
        };

        let stem = without_extension.rsplit('/').next().unwrap_or(without_extension);
        if stem.is_empty() {
            return Err(ModelPathError::EmptyName { path: path.to_string() });
        }

        Ok(Self(path.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The file name without `.model`, e.g. `AGM034_16in50_Mk7`.
    pub fn stem(&self) -> &str {
        let without_extension = &self.0[..self.0.len() - MODEL_EXTENSION.len()];

        without_extension.rsplit('/').next().unwrap_or(without_extension)
    }

    /// The directory holding the model, or `""` for a bare file name.
    pub fn directory(&self) -> &str {
        self.0.rsplit_once('/').map(|(dir, _)| dir).unwrap_or("")
    }

    /// The path of the sibling prototype with the same stem and `extension`
    /// (which includes the dot, e.g. `.visual`).
    pub fn with_extension(&self, extension: &str) -> String {
        format!("{}{extension}", &self.0[..self.0.len() - MODEL_EXTENSION.len()])
    }
}

impl fmt::Display for ModelPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Borrow<str> for ModelPath {
    fn borrow(&self) -> &str {
        &self.0
    }
}

/// The nation directory of a gameplay model, e.g. `usa`, `japan`, `events`,
/// `common`. These are directory names, not GameParams nation names.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ModelNation(String);

impl ModelNation {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ModelNation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A directory name, e.g. `building` or `Halloween19`, or a path of directory
/// names, e.g. `helpers/models`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ModelDirectory(String);

impl ModelDirectory {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ModelDirectory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The kind directory of a gameplay model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ModelKind {
    Ship,
    Gun,
    Director,
    Finder,
    Radar,
    Catapult,
    Aircraft,
    Projectile,
    Misc,
    Cap,
    /// A kind directory with no dedicated variant (e.g. `decoration`).
    Other,
}

impl ModelKind {
    /// Every kind, in display order.
    pub const ALL: [ModelKind; 11] = [
        Self::Ship,
        Self::Gun,
        Self::Director,
        Self::Finder,
        Self::Radar,
        Self::Catapult,
        Self::Aircraft,
        Self::Projectile,
        Self::Misc,
        Self::Cap,
        Self::Other,
    ];

    fn from_directory(directory: &str) -> Self {
        match directory {
            "ship" => Self::Ship,
            "gun" => Self::Gun,
            "director" => Self::Director,
            "finder" => Self::Finder,
            "radar" => Self::Radar,
            "catapult" => Self::Catapult,
            "aircraft" => Self::Aircraft,
            "projectile" => Self::Projectile,
            "misc" => Self::Misc,
            "cap" => Self::Cap,
            _ => Self::Other,
        }
    }
}

/// The sub-kind of a gameplay model: its sub-kind directory, read in the
/// context of its [`ModelKind`] (`torpedo` is a launcher under `gun` and a
/// weapon under `projectile`), or the [`MiscCategory`] of a misc model.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ModelSubKind {
    Battleship,
    Cruiser,
    Destroyer,
    AircraftCarrier,
    Submarine,
    Auxiliary,
    MainBattery,
    SecondaryBattery,
    AntiAircraft,
    TorpedoTubes,
    DepthChargeLauncher,
    MissileLauncher,
    SearchRadar,
    FireControlRadar,
    Fighter,
    Bomber,
    DiveBomber,
    Scout,
    Torpedo,
    Bomb,
    Rocket,
    DepthCharge,
    Mine,
    Missile,
    Jato,
    Shell,
    /// A misc model, which has no sub-kind directory, by what its name says it is.
    Misc(MiscCategory),
    /// A sub-kind directory this catalogue has no variant for.
    Unrecognized(String),
}

impl ModelSubKind {
    fn from_directory(kind: ModelKind, directory: &str) -> Self {
        let known = match (kind, directory) {
            (ModelKind::Ship, "battleship") => Some(Self::Battleship),
            (ModelKind::Ship, "cruiser") => Some(Self::Cruiser),
            (ModelKind::Ship, "destroyer") => Some(Self::Destroyer),
            (ModelKind::Ship, "aircarrier") => Some(Self::AircraftCarrier),
            (ModelKind::Ship, "submarine") => Some(Self::Submarine),
            (ModelKind::Ship, "auxiliary") => Some(Self::Auxiliary),
            (ModelKind::Gun, "main") => Some(Self::MainBattery),
            (ModelKind::Gun, "secondary") => Some(Self::SecondaryBattery),
            (ModelKind::Gun, "aaircraft") => Some(Self::AntiAircraft),
            (ModelKind::Gun, "torpedo") => Some(Self::TorpedoTubes),
            (ModelKind::Gun, "dcharge") => Some(Self::DepthChargeLauncher),
            (ModelKind::Gun, "missilegun") => Some(Self::MissileLauncher),
            (ModelKind::Radar, "search") => Some(Self::SearchRadar),
            (ModelKind::Radar, "fcontrol") => Some(Self::FireControlRadar),
            (ModelKind::Aircraft, "fighter") => Some(Self::Fighter),
            (ModelKind::Aircraft, "bomber") => Some(Self::Bomber),
            (ModelKind::Aircraft, "dive") => Some(Self::DiveBomber),
            (ModelKind::Aircraft, "scout") => Some(Self::Scout),
            (ModelKind::Projectile, "torpedo") => Some(Self::Torpedo),
            (ModelKind::Projectile, "bomb") => Some(Self::Bomb),
            (ModelKind::Projectile, "rocket") => Some(Self::Rocket),
            (ModelKind::Projectile, "depthcharge") => Some(Self::DepthCharge),
            (ModelKind::Projectile, "mine") => Some(Self::Mine),
            (ModelKind::Projectile, "missile") => Some(Self::Missile),
            (ModelKind::Projectile, "jato") => Some(Self::Jato),
            (ModelKind::Projectile, "artillery") => Some(Self::Shell),
            _ => None,
        };

        known.unwrap_or_else(|| Self::Unrecognized(directory.to_string()))
    }
}

/// What a gameplay misc model is, read from the English words of its file
/// name (`AM116_Boat_Winch_2` is a winch). Misc file names are the only signal
/// every misc model carries: the `MP_` nodes that place them are named after
/// the same file names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum MiscCategory {
    Crane,
    Anchor,
    DeckFitting,
    Instrument,
    Propeller,
    Searchlight,
    Boat,
    Flag,
    Insignia,
    Railing,
    Hatch,
    Ventilation,
    Antenna,
    Weapon,
    Stowage,
    /// No word of the name is in the keyword table.
    Other,
}

impl MiscCategory {
    /// Every category, in display order.
    pub const ALL: [MiscCategory; 16] = [
        Self::Propeller,
        Self::Boat,
        Self::Anchor,
        Self::Crane,
        Self::DeckFitting,
        Self::Searchlight,
        Self::Flag,
        Self::Insignia,
        Self::Railing,
        Self::Hatch,
        Self::Ventilation,
        Self::Antenna,
        Self::Instrument,
        Self::Weapon,
        Self::Stowage,
        Self::Other,
    ];

    /// The category of the misc model named `stem`, e.g. `AM116_Boat_Winch_2`.
    /// The first `_`-separated part is the model id and is skipped. The other
    /// parts are split into lowercase words at case and digit changes, and the
    /// first table row holding any of the words wins, so a name with two
    /// matching words (`Boat_Winch`) goes to the earlier row.
    pub fn classify(stem: &str) -> Self {
        const KEYWORDS: [(MiscCategory, &[&str]); 15] = [
            (MiscCategory::Crane, &["crane", "cranes", "cran", "davit", "davits", "derrick", "boom"]),
            (MiscCategory::Anchor, &["anchor", "anchors", "hawse", "chain"]),
            (
                MiscCategory::DeckFitting,
                &[
                    "capstan", "windlass", "winch", "winches", "bollard", "bollards", "fairlead", "fairled", "cleat",
                    "chock", "fender", "fenders", "roller", "bench", "seat",
                ],
            ),
            (
                MiscCategory::Instrument,
                &[
                    "rangefinder",
                    "rf",
                    "finder",
                    "director",
                    "dir",
                    "dct",
                    "binocular",
                    "binoculars",
                    "sight",
                    "periscope",
                    "compass",
                    "pelorus",
                    "binnacle",
                    "telegraph",
                    "indicator",
                    "ind",
                    "repeater",
                    "lookout",
                    "telephone",
                    "speaker",
                    "siren",
                    "gauge",
                    "manometer",
                    "anemometer",
                    "wheel",
                    "handwheel",
                    "steering",
                    "bell",
                    "clock",
                    "panel",
                    "switch",
                ],
            ),
            (MiscCategory::Propeller, &["propeller", "propellers", "screw", "screws", "rudder", "rudders", "shaft"]),
            (
                MiscCategory::Searchlight,
                &[
                    "searchlight",
                    "searchlights",
                    "projector",
                    "lamp",
                    "lamps",
                    "light",
                    "lights",
                    "lantern",
                    "lanterm",
                    "headlights",
                    "luminaire",
                    "fakel",
                    "mastheadlight",
                ],
            ),
            (
                MiscCategory::Boat,
                &[
                    "boat",
                    "boats",
                    "motorboat",
                    "motorboot",
                    "whaleboat",
                    "lifeboat",
                    "longboat",
                    "cutter",
                    "kutter",
                    "segelkutter",
                    "torpedoskutter",
                    "launch",
                    "lauch",
                    "dinghy",
                    "gig",
                    "pinnace",
                    "whaler",
                    "yawl",
                    "liferaft",
                    "raft",
                    "rafts",
                    "float",
                    "floater",
                    "motoscafi",
                    "motoscafo",
                    "lancia",
                    "lancie",
                    "bote",
                    "barca",
                    "canot",
                    "sampan",
                    "punt",
                    "skiff",
                    "shallop",
                    "lifebuoy",
                    "buoy",
                ],
            ),
            (MiscCategory::Flag, &["flag", "flags", "mast", "masts", "ensign", "pennant", "banner", "flagstaff"]),
            (
                MiscCategory::Insignia,
                &[
                    "crest",
                    "crests",
                    "emblem",
                    "nameplate",
                    "name",
                    "plate",
                    "monogramm",
                    "monogram",
                    "motto",
                    "honours",
                    "eagle",
                    "figure",
                    "figurehead",
                    "decal",
                    "decor",
                    "decoration",
                    "decorations",
                    "gerb",
                    "arms",
                    "sign",
                    "letter",
                    "text",
                    "ribbon",
                    "order",
                ],
            ),
            (
                MiscCategory::Railing,
                &[
                    "railing",
                    "railings",
                    "rail",
                    "rails",
                    "leer",
                    "leers",
                    "ladder",
                    "ladders",
                    "stairway",
                    "stairs",
                    "stair",
                    "gangway",
                    "accommodation",
                ],
            ),
            (MiscCategory::Hatch, &["hatch", "hatches", "door", "doors", "coaming", "manhole", "skylight"]),
            (
                MiscCategory::Ventilation,
                &[
                    "ventilation",
                    "ventelation",
                    "ventilator",
                    "ventilators",
                    "vent",
                    "vents",
                    "mushroom",
                    "cowl",
                    "intake",
                ],
            ),
            (MiscCategory::Antenna, &["radar", "antenna", "antennas", "aerial", "insulator", "insulators", "iff"]),
            (
                MiscCategory::Weapon,
                &[
                    "gun",
                    "guns",
                    "gundeck",
                    "barbette",
                    "cannon",
                    "kanon",
                    "turret",
                    "shield",
                    "torpedo",
                    "torpedoes",
                    "charge",
                    "thrower",
                    "launcher",
                    "paravane",
                    "paravanes",
                    "mine",
                    "mines",
                    "ammunition",
                    "ammobox",
                    "ammo",
                    "shell",
                    "shells",
                    "cartridges",
                    "loading",
                    "oerlikon",
                    "hotchkiss",
                    "pdr",
                    "mm",
                    "cm",
                    "catapult",
                    "rocket",
                ],
            ),
            (
                MiscCategory::Stowage,
                &[
                    "box",
                    "boxes",
                    "locker",
                    "lockers",
                    "crate",
                    "crates",
                    "barrel",
                    "barrels",
                    "bags",
                    "stowage",
                    "provisions",
                    "cart",
                    "shelving",
                    "rack",
                    "racks",
                    "basket",
                    "reel",
                    "reels",
                    "hose",
                    "drum",
                    "cylinder",
                    "spare",
                    "holder",
                    "cover",
                    "covers",
                    "cloth",
                    "tarpaulin",
                ],
            ),
        ];

        let words = name_words(stem);
        KEYWORDS
            .iter()
            .find(|(_, keywords)| words.iter().any(|word| keywords.contains(&word.as_str())))
            .map_or(Self::Other, |(category, _)| *category)
    }

    /// Lowercase identifier, e.g. `deck_fitting`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Crane => "crane",
            Self::Anchor => "anchor",
            Self::DeckFitting => "deck_fitting",
            Self::Instrument => "instrument",
            Self::Propeller => "propeller",
            Self::Searchlight => "searchlight",
            Self::Boat => "boat",
            Self::Flag => "flag",
            Self::Insignia => "insignia",
            Self::Railing => "railing",
            Self::Hatch => "hatch",
            Self::Ventilation => "ventilation",
            Self::Antenna => "antenna",
            Self::Weapon => "weapon",
            Self::Stowage => "stowage",
            Self::Other => "other",
        }
    }
}

/// What a GameParams `Building` is, from its species, plus the two groups of
/// building models that no species names.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum BuildingKind {
    Complex,
    CoastalArtillery,
    AntiAircraft,
    AirBase,
    Military,
    SensorTower,
    RayTower,
    Generator,
    SpaceStation,
    /// A species this catalogue has no variant for.
    Unrecognized(String),
    /// A param that names no species.
    Unspecified,
    /// A gun mount only buildings carry (`content/location/gameplay/gun/...`).
    Weapon,
    /// A model in a building directory that no `Building` param uses.
    Unassigned,
}

impl BuildingKind {
    pub fn from_species(species: &Recognized<Species>) -> Self {
        match species {
            Recognized::Known(Species::Complex) => Self::Complex,
            Recognized::Known(Species::CoastalArtillery) => Self::CoastalArtillery,
            Recognized::Known(Species::AntiAircraft) => Self::AntiAircraft,
            Recognized::Known(Species::AirBase) => Self::AirBase,
            Recognized::Known(Species::Military) => Self::Military,
            Recognized::Known(Species::SensorTower) => Self::SensorTower,
            Recognized::Known(Species::RayTower) => Self::RayTower,
            Recognized::Known(Species::Generator) => Self::Generator,
            Recognized::Known(Species::SpaceStation) => Self::SpaceStation,
            Recognized::Known(other) => Self::Unrecognized(other.name().to_string()),
            Recognized::Unknown(raw) => Self::Unrecognized(raw.clone()),
        }
    }

    /// The game's name for the species, `IDS_BUILDING_<SPECIES>` (e.g.
    /// `IDS_BUILDING_COMPLEX`, "Fort"). `None` for the kinds no species names.
    pub fn translation_key(&self) -> Option<TranslationKey> {
        let species = match self {
            Self::Complex => "COMPLEX",
            Self::CoastalArtillery => "COASTALARTILLERY",
            Self::AntiAircraft => "ANTIAIRCRAFT",
            Self::AirBase => "AIRBASE",
            Self::Military => "MILITARY",
            Self::SensorTower => "SENSORTOWER",
            Self::RayTower => "RAYTOWER",
            Self::Generator => "GENERATOR",
            Self::SpaceStation => "SPACESTATION",
            Self::Unrecognized(raw) => {
                return Some(TranslationKey::new(format!("IDS_BUILDING_{}", raw.to_uppercase())));
            }
            Self::Unspecified | Self::Weapon | Self::Unassigned => return None,
        };

        Some(TranslationKey::new(format!("IDS_BUILDING_{species}")))
    }
}

/// A GameParams `Building` param, e.g. `PCBX022_PCGM002`, a fort.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BuildingParamRef {
    name: String,
    kind: BuildingKind,
}

impl BuildingParamRef {
    pub fn new(name: String, kind: BuildingKind) -> Self {
        Self { name, kind }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn kind(&self) -> &BuildingKind {
        &self.kind
    }
}

/// The models one `Building` param uses, as GameParams spells their paths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildingModels {
    pub param: BuildingParamRef,
    /// `hull.model`.
    pub model: Option<String>,
    /// `hull.deadModel`.
    pub dead_model: Option<String>,
    /// The `model` and `deadMesh` of each `HP_` mount.
    pub mount_models: Vec<String>,
}

/// The links between models that GameParams declares.
#[derive(Debug, Clone, Default)]
pub struct CatalogLinks {
    skins: Vec<PeculiarityModel>,
    buildings: Vec<BuildingModels>,
}

impl CatalogLinks {
    pub fn new(skins: Vec<PeculiarityModel>, buildings: Vec<BuildingModels>) -> Self {
        Self { skins, buildings }
    }

    pub fn from_metadata(metadata: &GameMetadataProvider) -> Self {
        Self::new(peculiarity_models(metadata), building_models(metadata))
    }

    pub fn skins(&self) -> &[PeculiarityModel] {
        &self.skins
    }

    pub fn buildings(&self) -> &[BuildingModels] {
        &self.buildings
    }
}

/// Where a gameplay model sits in its directory layout.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct GameplayGroup {
    nation: ModelNation,
    kind: ModelKind,
    sub_kind: Option<ModelSubKind>,
}

impl GameplayGroup {
    pub fn nation(&self) -> &ModelNation {
        &self.nation
    }

    pub fn kind(&self) -> ModelKind {
        self.kind
    }

    pub fn sub_kind(&self) -> Option<&ModelSubKind> {
        self.sub_kind.as_ref()
    }
}

/// The category directories of a location or port model, e.g. `building` and
/// `city` for `content/location/building/city/<dir>/<leaf>.model`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SceneGroup {
    category: ModelDirectory,
    sub_category: Option<ModelDirectory>,
}

impl SceneGroup {
    pub fn category(&self) -> &ModelDirectory {
        &self.category
    }

    pub fn sub_category(&self) -> Option<&ModelDirectory> {
        self.sub_category.as_ref()
    }
}

/// The folder a model is listed in.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ModelGroup {
    /// `content/gameplay/<nation>/<kind>/[<sub-kind>/]<dir>/<leaf>.model`.
    Gameplay(GameplayGroup),
    /// A model GameParams `Building` params use, or one in a building directory.
    Building(BuildingKind),
    /// `content/gameplay/<nation>/character/<dir>/<leaf>.model`.
    Character(ModelNation),
    /// `content/location/<category>/[<sub-category>/]<dir>/<leaf>.model`.
    Location(SceneGroup),
    /// `content/ports/<category>/[<sub-category>/]<dir>/<leaf>.model`.
    Port(SceneGroup),
    /// `content/styles/<style>/<dir>/<leaf>.model`.
    Style(ModelDirectory),
    /// Any other model, by its first two directories (`helpers/models`), or
    /// `None` for a model at the VFS root.
    Other(Option<ModelDirectory>),
}

impl ModelGroup {
    pub fn kind(&self) -> ModelGroupKind {
        match self {
            Self::Gameplay(_) => ModelGroupKind::Gameplay,
            Self::Building(_) => ModelGroupKind::Building,
            Self::Character(_) => ModelGroupKind::Character,
            Self::Location(_) => ModelGroupKind::Location,
            Self::Port(_) => ModelGroupKind::Port,
            Self::Style(_) => ModelGroupKind::Style,
            Self::Other(_) => ModelGroupKind::Other,
        }
    }

    pub fn gameplay(&self) -> Option<&GameplayGroup> {
        match self {
            Self::Gameplay(group) => Some(group),
            _ => None,
        }
    }
}

/// The top-level folder of a [`ModelGroup`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ModelGroupKind {
    Gameplay,
    Building,
    Character,
    Location,
    Port,
    Style,
    Other,
}

impl ModelGroupKind {
    /// Every top-level folder, in display order.
    pub const ALL: [ModelGroupKind; 7] =
        [Self::Gameplay, Self::Building, Self::Character, Self::Location, Self::Port, Self::Style, Self::Other];
}

/// Whether the install ships the geometry a model's visual names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum GeometryAvailability {
    Present,
    /// The visual names a geometry file the installed packages do not hold.
    Absent,
    /// The visual names no geometry, as placeholder models do.
    NoGeometry,
}

/// How a variant relates to its base model.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ModelVariantKind {
    /// The wreck model (`<base>_dead.model`, or a building's `deadModel`).
    Dead,
    /// A sibling `<base>_<suffix>.model`, e.g. the `Bow` hull section.
    Section(String),
    /// A model an exterior shows in place of the base (`peculiarityModels`).
    Skin,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ModelVariant {
    pub kind: ModelVariantKind,
    pub path: ModelPath,
}

/// One base model of the catalogue.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ModelEntry {
    path: ModelPath,
    group: ModelGroup,
    variants: Vec<ModelVariant>,
    skin_of: Vec<ModelPath>,
    building_params: Vec<BuildingParamRef>,
}

impl ModelEntry {
    pub fn path(&self) -> &ModelPath {
        &self.path
    }

    /// The model's id, its file stem, e.g. `AGM034_16in50_Mk7`.
    pub fn name(&self) -> &str {
        self.path.stem()
    }

    pub fn group(&self) -> &ModelGroup {
        &self.group
    }

    /// Dead, section and skin variants, sorted by kind then path.
    pub fn variants(&self) -> &[ModelVariant] {
        &self.variants
    }

    /// The hull sections a ship model is drawn from, in path order.
    pub fn sections(&self) -> impl Iterator<Item = &ModelPath> {
        self.variants.iter().filter(|v| matches!(v.kind, ModelVariantKind::Section(_))).map(|v| &v.path)
    }

    /// Stock models this model is a skin of. A non-empty list means a model
    /// browser lists this entry under those models rather than on its own.
    pub fn skin_of(&self) -> &[ModelPath] {
        &self.skin_of
    }

    /// The `Building` params that use this model or one of its dead and
    /// section variants as hull, wreck or mount, sorted.
    pub fn building_params(&self) -> &[BuildingParamRef] {
        &self.building_params
    }

    /// The part group a ship lists a mount of this model under.
    pub fn part_group(&self) -> PartGroup {
        let ModelGroup::Gameplay(group) = &self.group else {
            return PartGroup::Other;
        };

        match (group.kind, &group.sub_kind) {
            (ModelKind::Ship, _) => PartGroup::Hull,
            (ModelKind::Gun, Some(ModelSubKind::MainBattery)) => PartGroup::MainBattery,
            (ModelKind::Gun, Some(ModelSubKind::SecondaryBattery)) => PartGroup::SecondaryBattery,
            (ModelKind::Gun, Some(ModelSubKind::AntiAircraft)) => PartGroup::AaGuns,
            (ModelKind::Gun, Some(ModelSubKind::TorpedoTubes)) => PartGroup::Torpedoes,
            (ModelKind::Gun, Some(ModelSubKind::DepthChargeLauncher)) => PartGroup::DepthCharges,
            (ModelKind::Gun, Some(ModelSubKind::MissileLauncher)) => PartGroup::Missiles,
            (ModelKind::Director | ModelKind::Finder, _) => PartGroup::FireControl,
            (ModelKind::Radar, _) => PartGroup::Radar,
            (ModelKind::Misc, _) => PartGroup::Misc,
            _ => PartGroup::Other,
        }
    }
}

/// Every model in an `assets.bin`, grouped for browsing.
#[derive(Debug, Clone, Default)]
pub struct ModelCatalog {
    /// Base models, sorted by path.
    entries: Vec<ModelEntry>,
    entry_index: HashMap<ModelPath, usize>,
    /// Dead and section variant path to the index of its base entry.
    variant_owner: HashMap<ModelPath, usize>,
    group_counts: BTreeMap<ModelGroupKind, usize>,
    /// Per model path; empty until [`Self::with_geometry`].
    geometry: HashMap<ModelPath, GeometryAvailability>,
}

impl ModelCatalog {
    /// Group `paths` and apply the GameParams `links`. Every path becomes a
    /// base entry or a dead or section variant of one. A skin pair whose stock
    /// or replacement is not a base model of the catalogue is skipped.
    pub fn from_paths(paths: impl IntoIterator<Item = ModelPath>, links: &CatalogLinks) -> Self {
        let mut by_directory: BTreeMap<String, Vec<ModelPath>> = BTreeMap::new();
        for path in paths {
            by_directory.entry(path.directory().to_string()).or_default().push(path);
        }

        let mut bases: Vec<ModelPath> = Vec::new();
        let mut pending_variants: Vec<(ModelPath, ModelVariant)> = Vec::new();
        for (directory, mut paths) in by_directory {
            paths.sort();
            paths.dedup();
            let directory_name = directory.rsplit('/').next().unwrap_or(&directory);

            let Some(base) = paths.iter().find(|p| p.stem() == directory_name).cloned() else {
                bases.extend(paths);
                continue;
            };

            for path in paths {
                if path == base {
                    continue;
                }
                match variant_kind(directory_name, path.stem()) {
                    Some(kind) => pending_variants.push((base.clone(), ModelVariant { kind, path })),
                    None => bases.push(path),
                }
            }
            bases.push(base);
        }

        let buildings = BuildingUses::new(links.buildings());
        let mut bases: HashSet<ModelPath> = bases.into_iter().collect();
        for (hull, dead) in buildings.separate_wrecks(&bases) {
            bases.remove(&dead);
            // The wreck's own parts are wreck models of the hull, never parts it loads with.
            for (owner, variant) in pending_variants.iter_mut().filter(|(owner, _)| *owner == dead) {
                *owner = hull.clone();
                variant.kind = ModelVariantKind::Dead;
            }
            pending_variants.push((hull, ModelVariant { kind: ModelVariantKind::Dead, path: dead }));
        }

        let mut owner_of: HashMap<&ModelPath, &ModelPath> = bases.iter().map(|base| (base, base)).collect();
        for (base, variant) in &pending_variants {
            owner_of.insert(&variant.path, base);
        }
        let mut votes: HashMap<&ModelPath, BuildingVotes> = HashMap::new();
        for (path, role, param) in buildings.references() {
            let Some(&owner) = owner_of.get(path) else {
                continue;
            };
            votes.entry(owner).or_default().add(role, param);
        }

        let mut entries: Vec<ModelEntry> = bases
            .iter()
            .map(|path| {
                let votes = votes.get(path);
                ModelEntry {
                    path: path.clone(),
                    group: group_of(path, votes),
                    variants: Vec::new(),
                    skin_of: Vec::new(),
                    building_params: votes.map(BuildingVotes::params).unwrap_or_default(),
                }
            })
            .collect();
        entries.sort_by(|a, b| a.path.cmp(&b.path));
        let entry_index: HashMap<ModelPath, usize> =
            entries.iter().enumerate().map(|(idx, entry)| (entry.path.clone(), idx)).collect();

        let mut variant_owner = HashMap::new();
        for (base, variant) in pending_variants {
            let Some(&owner) = entry_index.get(&base) else {
                continue;
            };
            variant_owner.insert(variant.path.clone(), owner);
            entries[owner].variants.push(variant);
        }

        for pair in links.skins() {
            let (Ok(stock), Ok(skin)) = (ModelPath::parse(&pair.stock), ModelPath::parse(&pair.replacement)) else {
                continue;
            };
            let (Some(&stock_idx), Some(&skin_idx)) = (entry_index.get(&stock), entry_index.get(&skin)) else {
                continue;
            };
            if stock_idx == skin_idx {
                continue;
            }

            let skin_variant = ModelVariant { kind: ModelVariantKind::Skin, path: skin };
            if !entries[stock_idx].variants.contains(&skin_variant) {
                entries[stock_idx].variants.push(skin_variant);
            }
            if !entries[skin_idx].skin_of.contains(&stock) {
                entries[skin_idx].skin_of.push(stock);
            }
        }

        let mut group_counts = BTreeMap::new();
        for entry in &mut entries {
            entry.variants.sort_by(|a, b| (&a.kind, &a.path).cmp(&(&b.kind, &b.path)));
            entry.skin_of.sort();
            *group_counts.entry(entry.group.kind()).or_insert(0) += 1;
        }

        Self { entries, entry_index, variant_owner, group_counts, geometry: HashMap::new() }
    }

    /// Every base model, skins included, sorted by path.
    pub fn entries(&self) -> &[ModelEntry] {
        &self.entries
    }

    /// The base models a browser lists at top level: every entry that is not
    /// a skin of another model.
    pub fn listed_entries(&self) -> impl Iterator<Item = &ModelEntry> {
        self.entries.iter().filter(|entry| entry.skin_of.is_empty())
    }

    /// The base entry for `path`, which must name a base model.
    pub fn entry(&self, path: &ModelPath) -> Option<&ModelEntry> {
        self.entry_index.get(path).map(|&idx| &self.entries[idx])
    }

    /// The base entry `path` belongs to: itself for a base model, its owner for
    /// a dead or section variant.
    pub fn owning_entry(&self, path: &ModelPath) -> Option<&ModelEntry> {
        self.entry(path).or_else(|| self.variant_owner.get(path).map(|&idx| &self.entries[idx]))
    }

    /// Number of base models per top-level folder, skins included.
    pub fn count_by_group(&self) -> &BTreeMap<ModelGroupKind, usize> {
        &self.group_counts
    }

    /// Attach the geometry availability of each model path.
    pub fn with_geometry(mut self, geometry: HashMap<ModelPath, GeometryAvailability>) -> Self {
        self.geometry = geometry;
        self
    }

    /// Whether `path` can be drawn. A base model loads with its hull sections,
    /// so it takes the first part, in load order, that is not present. `None`
    /// when the availability of a part before that is unknown.
    pub fn geometry(&self, path: &ModelPath) -> Option<GeometryAvailability> {
        let sections = self.entry(path).into_iter().flat_map(ModelEntry::sections);
        for part in std::iter::once(path).chain(sections) {
            let availability = *self.geometry.get(part)?;
            if availability != GeometryAvailability::Present {
                return Some(availability);
            }
        }

        Some(GeometryAvailability::Present)
    }
}

/// Paths-storage position of every `.model` prototype in an `assets.bin`, so a
/// model is found by path without scanning the store.
#[derive(Debug, Clone, Default)]
pub struct ModelPathIndex {
    positions: BTreeMap<ModelPath, usize>,
}

impl ModelPathIndex {
    /// One pass over the paths store. `self_id_index` is
    /// [`PrototypeDatabase::build_self_id_index`] of the same database.
    pub fn build(db: &PrototypeDatabase<'_>, self_id_index: &HashMap<u64, usize>) -> Self {
        let mut positions = BTreeMap::new();
        for (position, entry) in db.paths_storage.iter().enumerate() {
            if !entry.name.ends_with(MODEL_EXTENSION) {
                continue;
            }
            let is_model_record =
                db.lookup_r2p(entry.self_id).and_then(|value| db.decode_r2p_value(value).ok()).is_some_and(
                    |location| PrototypeType::from_blob_index(location.blob_index) == Some(PrototypeType::Model),
                );
            if !is_model_record {
                continue;
            }
            if let Ok(path) = ModelPath::parse(&db.reconstruct_path(position, self_id_index)) {
                positions.insert(path, position);
            }
        }

        Self { positions }
    }

    /// Index of the model's entry in `PrototypeDatabase::paths_storage`.
    pub fn position(&self, path: &ModelPath) -> Option<usize> {
        self.positions.get(path).copied()
    }

    pub fn paths(&self) -> impl Iterator<Item = &ModelPath> {
        self.positions.keys()
    }

    pub fn len(&self) -> usize {
        self.positions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.positions.is_empty()
    }
}

/// How a `Building` param uses a model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BuildingRole {
    Hull,
    Wreck,
    Mount,
}

/// Every model path the `Building` params name, by role.
struct BuildingUses<'a> {
    references: Vec<(ModelPath, BuildingRole, &'a BuildingParamRef)>,
    /// `(hull, deadModel)` pairs, in param order.
    wrecks: Vec<(ModelPath, ModelPath)>,
}

impl<'a> BuildingUses<'a> {
    fn new(buildings: &'a [BuildingModels]) -> Self {
        let mut buildings: Vec<&BuildingModels> = buildings.iter().collect();
        buildings.sort_by(|a, b| a.param.cmp(&b.param));

        let parse = |raw: &Option<String>| raw.as_deref().and_then(|raw| ModelPath::parse(raw).ok());
        let mut references = Vec::new();
        let mut wrecks = Vec::new();
        for building in buildings {
            let hull = parse(&building.model);
            let dead = parse(&building.dead_model);
            if let (Some(hull), Some(dead)) = (&hull, &dead)
                && hull != dead
            {
                wrecks.push((hull.clone(), dead.clone()));
            }
            if let Some(hull) = hull {
                references.push((hull, BuildingRole::Hull, &building.param));
            }
            if let Some(dead) = dead {
                references.push((dead, BuildingRole::Wreck, &building.param));
            }
            for mount in &building.mount_models {
                if let Ok(mount) = ModelPath::parse(mount) {
                    references.push((mount, BuildingRole::Mount, &building.param));
                }
            }
        }

        Self { references, wrecks }
    }

    fn references(&self) -> impl Iterator<Item = (&ModelPath, BuildingRole, &'a BuildingParamRef)> {
        self.references.iter().map(|(path, role, param)| (path, *role, *param))
    }

    /// `(hull, wreck)` pairs where the wreck is a base model of its own
    /// directory that no building uses as its hull, so it is listed as the
    /// hull's dead variant. A wreck claimed by several hulls goes to the first.
    fn separate_wrecks(&self, bases: &HashSet<ModelPath>) -> Vec<(ModelPath, ModelPath)> {
        let hulls: HashSet<&ModelPath> = self
            .references
            .iter()
            .filter(|(_, role, _)| *role == BuildingRole::Hull)
            .map(|(path, _, _)| path)
            .collect();

        let mut claimed: HashSet<&ModelPath> = HashSet::new();
        let mut pairs = Vec::new();
        for (hull, dead) in &self.wrecks {
            if hulls.contains(dead) || !bases.contains(hull) || !bases.contains(dead) || !claimed.insert(dead) {
                continue;
            }
            pairs.push((hull.clone(), dead.clone()));
        }

        pairs
    }
}

/// The building params that reference one base model, by role.
#[derive(Default)]
struct BuildingVotes {
    hull: Vec<BuildingParamRef>,
    wreck: Vec<BuildingParamRef>,
    mount: Vec<BuildingParamRef>,
}

impl BuildingVotes {
    fn add(&mut self, role: BuildingRole, param: &BuildingParamRef) {
        let list = match role {
            BuildingRole::Hull => &mut self.hull,
            BuildingRole::Wreck => &mut self.wreck,
            BuildingRole::Mount => &mut self.mount,
        };
        list.push(param.clone());
    }

    /// The kind most hull uses name, else most wreck uses; ties go to the
    /// earlier kind, and params without a species count only when no other
    /// does. `None` when the model is only a mount.
    fn structure_kind(&self) -> Option<BuildingKind> {
        let users = if self.hull.is_empty() { &self.wreck } else { &self.hull };
        let mut counts: BTreeMap<&BuildingKind, usize> = BTreeMap::new();
        for param in users.iter().filter(|param| param.kind != BuildingKind::Unspecified) {
            *counts.entry(&param.kind).or_insert(0) += 1;
        }

        let Some(most) = counts.values().copied().max() else {
            return (!users.is_empty()).then_some(BuildingKind::Unspecified);
        };
        counts.into_iter().find(|(_, count)| *count == most).map(|(kind, _)| kind.clone())
    }

    fn is_mount(&self) -> bool {
        !self.mount.is_empty()
    }

    fn params(&self) -> Vec<BuildingParamRef> {
        let mut params: Vec<BuildingParamRef> =
            self.hull.iter().chain(&self.wreck).chain(&self.mount).cloned().collect();
        params.sort();
        params.dedup();

        params
    }
}

/// Every `peculiarityModels` pair across the GameParams exteriors, sorted and
/// without duplicates.
pub fn peculiarity_models(metadata: &GameMetadataProvider) -> Vec<PeculiarityModel> {
    let mut pairs: Vec<PeculiarityModel> = metadata
        .params()
        .iter()
        .filter_map(|param| param.exterior())
        .flat_map(|exterior| exterior.peculiarity_models().iter().cloned())
        .collect();
    pairs.sort();
    pairs.dedup();

    pairs
}

/// The models of every GameParams `Building` param.
pub fn building_models(metadata: &GameMetadataProvider) -> Vec<BuildingModels> {
    metadata
        .params()
        .iter()
        .filter_map(|param| {
            let building = param.building()?;
            let kind = param.species().map_or(BuildingKind::Unspecified, BuildingKind::from_species);
            let mounts = building.mounts().iter().map(|mount| mount.model_path().to_string());

            Some(BuildingModels {
                param: BuildingParamRef::new(param.name().to_string(), kind),
                model: building.model().map(str::to_string),
                dead_model: building.dead_model().map(str::to_string),
                mount_models: mounts.chain(building.mount_dead_models().iter().cloned()).collect(),
            })
        })
        .collect()
}

/// The folder for the base model `path`. A model a building uses as hull or
/// wreck goes under that building's kind, and a building-only mount under
/// [`BuildingKind::Weapon`], ahead of the path layout.
fn group_of(path: &ModelPath, votes: Option<&BuildingVotes>) -> ModelGroup {
    if let Some(kind) = votes.and_then(BuildingVotes::structure_kind) {
        return ModelGroup::Building(kind);
    }

    let raw = path.as_str();
    if votes.is_some_and(BuildingVotes::is_mount) && !raw.starts_with(GAMEPLAY_PREFIX) {
        return ModelGroup::Building(BuildingKind::Weapon);
    }

    // Building directories, whether or not a current param uses the model.
    if raw.starts_with("content/location/gameplay/") || raw.starts_with("content/location/nature/gameplay/") {
        return ModelGroup::Building(BuildingKind::Unassigned);
    }

    let directories: Vec<&str> = path.directory().split('/').collect();
    let named = |dirs: &[&str]| dirs.iter().all(|dir| !dir.is_empty());

    if raw.starts_with(GAMEPLAY_PREFIX) {
        let dirs = &directories[2..];
        if (3..=4).contains(&dirs.len()) && named(dirs) {
            return gameplay_group(dirs, path.stem());
        }
    }

    let scene = || {
        let dirs = &directories[2..];
        ((2..=3).contains(&dirs.len()) && named(dirs)).then(|| SceneGroup {
            category: ModelDirectory(dirs[0].to_string()),
            sub_category: (dirs.len() == 3).then(|| ModelDirectory(dirs[1].to_string())),
        })
    };
    if raw.starts_with(LOCATION_PREFIX)
        && let Some(scene) = scene()
    {
        return ModelGroup::Location(scene);
    }
    if raw.starts_with(PORTS_PREFIX)
        && let Some(scene) = scene()
    {
        return ModelGroup::Port(scene);
    }

    if raw.starts_with(STYLES_PREFIX) {
        let dirs = &directories[2..];
        if dirs.len() == 2 && named(dirs) {
            return ModelGroup::Style(ModelDirectory(dirs[0].to_string()));
        }
    }

    let top: Vec<&str> = directories.iter().take(2).copied().filter(|dir| !dir.is_empty()).collect();
    ModelGroup::Other((!top.is_empty()).then(|| ModelDirectory(top.join("/"))))
}

/// The group for a gameplay model below `content/gameplay/` whose directories
/// are `dirs`: `<nation>/<kind>/[<sub-kind>/]<dir>`.
fn gameplay_group(dirs: &[&str], stem: &str) -> ModelGroup {
    let nation = ModelNation(dirs[0].to_string());
    if dirs[1] == "character" {
        return ModelGroup::Character(nation);
    }

    let kind = ModelKind::from_directory(dirs[1]);
    let sub_kind = match (dirs.len(), kind) {
        (4, _) => Some(ModelSubKind::from_directory(kind, dirs[2])),
        (_, ModelKind::Misc) => Some(ModelSubKind::Misc(MiscCategory::classify(stem))),
        _ => None,
    };

    ModelGroup::Gameplay(GameplayGroup { nation, kind, sub_kind })
}

/// The lowercase English words of a model name after its leading id: `_`
/// separates parts, and a part splits where letters meet digits and where a
/// lowercase letter meets an uppercase one (`MotorBoat35ft` is `motor`,
/// `boat`, `35`, `ft`).
fn name_words(stem: &str) -> Vec<String> {
    let mut words = Vec::new();
    for part in stem.split('_').skip(1) {
        let mut word = String::new();
        let mut previous: Option<char> = None;
        for ch in part.chars() {
            let splits = previous.is_some_and(|prev| {
                prev.is_ascii_digit() != ch.is_ascii_digit() || (prev.is_lowercase() && ch.is_uppercase())
            });
            if splits && !word.is_empty() {
                words.push(std::mem::take(&mut word));
            }
            word.extend(ch.to_lowercase());
            previous = Some(ch);
        }
        if !word.is_empty() {
            words.push(word);
        }
    }

    words
}

/// The variant a sibling of the base model in `directory_name` is, or `None`
/// for an unrelated model sharing the directory.
fn variant_kind(directory_name: &str, stem: &str) -> Option<ModelVariantKind> {
    if stem.ends_with(DEAD_SUFFIX) {
        return Some(ModelVariantKind::Dead);
    }

    stem.strip_prefix(directory_name)
        .and_then(|rest| rest.strip_prefix('_'))
        .filter(|suffix| !suffix.is_empty())
        .map(|suffix| ModelVariantKind::Section(suffix.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(raw: &str) -> ModelPath {
        ModelPath::parse(raw).expect("valid model path")
    }

    fn skins(pairs: &[(&str, &str)]) -> Vec<PeculiarityModel> {
        pairs
            .iter()
            .map(|(stock, replacement)| PeculiarityModel {
                stock: stock.to_string(),
                replacement: replacement.to_string(),
            })
            .collect()
    }

    fn catalog(raw: &[&str], pairs: &[(&str, &str)]) -> ModelCatalog {
        ModelCatalog::from_paths(raw.iter().map(|p| path(p)), &CatalogLinks::new(skins(pairs), Vec::new()))
    }

    fn building(name: &str, kind: BuildingKind, model: &str, dead_model: &str, mounts: &[&str]) -> BuildingModels {
        let optional = |raw: &str| (!raw.is_empty()).then(|| raw.to_string());

        BuildingModels {
            param: BuildingParamRef::new(name.to_string(), kind),
            model: optional(model),
            dead_model: optional(dead_model),
            mount_models: mounts.iter().map(|mount| mount.to_string()).collect(),
        }
    }

    fn summary(catalog: &ModelCatalog) -> Vec<String> {
        catalog.entries().iter().map(|e| format!("{} | {:?} | {:?}", e.name(), e.group(), e.part_group())).collect()
    }

    #[test]
    fn model_paths_need_a_model_file_name() {
        assert_eq!(
            ModelPath::parse("content/a/b.visual"),
            Err(ModelPathError::NotAModel { path: "content/a/b.visual".to_string() })
        );
        assert_eq!(
            ModelPath::parse("content/a/.model"),
            Err(ModelPathError::EmptyName { path: "content/a/.model".to_string() })
        );

        let model = path("/content/gameplay/usa/misc/AM2061/AM2061.model");
        assert_eq!(model.as_str(), "content/gameplay/usa/misc/AM2061/AM2061.model");
        assert_eq!(model.stem(), "AM2061");
        assert_eq!(model.directory(), "content/gameplay/usa/misc/AM2061");
        assert_eq!(model.with_extension(".visual"), "content/gameplay/usa/misc/AM2061/AM2061.visual");
    }

    #[test]
    fn groups_real_paths_by_nation_kind_and_sub_kind() {
        let catalog = catalog(
            &[
                "content/gameplay/usa/ship/battleship/ASB028_Iowa_1945/ASB028_Iowa_1945.model",
                "content/gameplay/japan/gun/main/JGM178_460mm_45_Type94/JGM178_460mm_45_Type94.model",
                "content/gameplay/usa/gun/secondary/AGS010_5in38_Mk32_Mod12/AGS010_5in38_Mk32_Mod12.model",
                "content/gameplay/usa/gun/aaircraft/AGA002_40mm_Bofors_Mk1/AGA002_40mm_Bofors_Mk1.model",
                "content/gameplay/japan/gun/torpedo/JGT012_610mm4_Type_93/JGT012_610mm4_Type_93.model",
                "content/gameplay/japan/gun/dcharge/JGB107_Depth_Charge_Thrower_450mm/JGB107_Depth_Charge_Thrower_450mm.model",
                "content/gameplay/usa/director/AD001_Director_Mk37/AD001_Director_Mk37.model",
                "content/gameplay/usa/finder/AF312_Rangefinder_18ft/AF312_Rangefinder_18ft.model",
                "content/gameplay/usa/radar/search/ARS310_Radar/ARS310_Radar.model",
                "content/gameplay/usa/radar/fcontrol/ARF322_Radar_Mk27_Black/ARF322_Radar_Mk27_Black.model",
                "content/gameplay/usa/catapult/AC001_Catapult_1/AC001_Catapult_1.model",
                "content/gameplay/usa/aircraft/fighter/AAF001_Grumman_F3F/AAF001_Grumman_F3F.model",
                "content/gameplay/usa/projectile/torpedo/APT003_Avia_Torpedo_Mk13/APT003_Avia_Torpedo_Mk13.model",
                "content/gameplay/usa/projectile/jato/APJ001_Jato_14AS_1000/APJ001_Jato_14AS_1000.model",
                "content/gameplay/usa/misc/AM116_Boat_Winch_2/AM116_Boat_Winch_2.model",
                "content/gameplay/usa/misc/AM2061/AM2061.model",
                "content/gameplay/events/director/XD006_Lazarus_Director/XD006_Lazarus_Director.model",
                "content/gameplay/common/decoration/CD001_Garland/CD001_Garland.model",
                "content/gameplay/common/character/CH001_Dasha/CH001_Dasha.model",
                "content/gameplay/usa/gun/railgun/AGR001_Test/AGR001_Test.model",
            ],
            &[],
        );

        insta::assert_debug_snapshot!(summary(&catalog));
    }

    #[test]
    fn non_gameplay_paths_group_by_their_directories() {
        let catalog = catalog(
            &[
                "content/location/building/city/LBC001_House/LBC001_House.model",
                "content/location/landmark/LL371_Statue_of_liberty/LL371_Statue_of_liberty.model",
                "content/location/nature/rock/LNR561_Shoreside/LNR561_Shoreside.model",
                "content/location/gameplay/building/LYB206_Respawn/LYB206_Respawn.model",
                "content/location/nature/gameplay/LNG006/LNG006.model",
                "content/ports/misc/wharf/OMW001_Bollard/OMW001_Bollard.model",
                "content/ports/landmark/OL160_London/OL160_London.model",
                "content/styles/Halloween19/YMS001_YHBM_M_1/YMS001_YHBM_M_1.model",
                "content/pbs_test/box/box.model",
                "content/pbs_test/GGX_artists_materials_set/02_aluminum.model",
                "helpers/models/unit_cube.model",
                "content/mesh_decal_test/pillar.model",
                "unit_sphere.model",
                // Off the gameplay layout: too shallow, too deep, an empty directory.
                "content/gameplay/usa/AM1.model",
                "content/gameplay/usa/gun/main/extra/AGX/AGX.model",
                "content/gameplay//misc/AM3/AM3.model",
                // Too deep for a scene layout.
                "content/location/building/city/extra/LBC9/LBC9.model",
            ],
            &[],
        );

        insta::assert_debug_snapshot!(summary(&catalog));
    }

    #[test]
    fn dead_and_section_models_attach_to_their_base() {
        let catalog = catalog(
            &[
                "content/gameplay/usa/ship/battleship/ASB028_Iowa_1945/ASB028_Iowa_1945.model",
                "content/gameplay/usa/ship/battleship/ASB028_Iowa_1945/ASB028_Iowa_1945_Bow.model",
                "content/gameplay/usa/ship/battleship/ASB028_Iowa_1945/ASB028_Iowa_1945_MidFront.model",
                "content/gameplay/usa/ship/battleship/ASB028_Iowa_1945/ASB028_Iowa_1945_MidBack.model",
                "content/gameplay/usa/ship/battleship/ASB028_Iowa_1945/ASB028_Iowa_1945_Stern.model",
                "content/gameplay/usa/gun/main/AGM034_16in50_Mk7/AGM034_16in50_Mk7.model",
                "content/gameplay/usa/gun/main/AGM034_16in50_Mk7/AGM034_16in50_Mk7_dead.model",
                // A dead model whose stem does not repeat the directory name.
                "content/gameplay/russia/misc/RM492_Crane_Mount/RM492_Crane_Mount.model",
                "content/gameplay/russia/misc/RM492_Crane_Mount/RM492_dead.model",
                // A directory without a base model keeps every model listed.
                "content/gameplay/usa/misc/AM900_Set/AM901_Part.model",
                "content/gameplay/usa/misc/AM900_Set/AM902_Part.model",
                // The same rules hold outside gameplay.
                "content/location/vehicle/railroad/LVR001_Wagon/LVR001_Wagon.model",
                "content/location/vehicle/railroad/LVR001_Wagon/LVR001_Wagon_dead.model",
            ],
            &[],
        );

        let listed: Vec<(&str, &[ModelVariant])> = catalog.listed_entries().map(|e| (e.name(), e.variants())).collect();
        insta::assert_debug_snapshot!(listed);

        let bow = path("content/gameplay/usa/ship/battleship/ASB028_Iowa_1945/ASB028_Iowa_1945_Bow.model");
        assert_eq!(catalog.entry(&bow), None, "a section is not a base model");
        assert_eq!(catalog.owning_entry(&bow).map(ModelEntry::name), Some("ASB028_Iowa_1945"));
        let iowa = catalog.owning_entry(&bow).expect("Iowa entry");
        assert_eq!(iowa.sections().count(), 4);
    }

    #[test]
    fn skins_nest_under_their_stock_model() {
        let stock = "content/gameplay/germany/gun/main/GGM058_280mm54_5_C34/GGM058_280mm54_5_C34.model";
        let stock_dead = "content/gameplay/germany/gun/main/GGM058_280mm54_5_C34/GGM058_280mm54_5_C34_dead.model";
        let skin = "content/gameplay/germany/gun/main/GGM546_280mm54_5_C34_Black/GGM546_280mm54_5_C34_Black.model";
        let skin_dead =
            "content/gameplay/germany/gun/main/GGM546_280mm54_5_C34_Black/GGM546_280mm54_5_C34_Black_dead.model";
        let catalog = catalog(
            &[stock, stock_dead, skin, skin_dead],
            &[
                (stock, skin),
                // A pair between variants links nothing: the bases already do.
                (stock_dead, skin_dead),
                // A pair naming a model absent from the catalogue is skipped.
                (stock, "content/gameplay/germany/gun/main/GGM999_Missing/GGM999_Missing.model"),
            ],
        );

        let listed: Vec<&str> = catalog.listed_entries().map(ModelEntry::name).collect();
        assert_eq!(listed, ["GGM058_280mm54_5_C34"], "the skin is listed under its stock model only");

        let stock_entry = catalog.entry(&path(stock)).expect("stock entry");
        let kinds: Vec<(&ModelVariantKind, &str)> =
            stock_entry.variants().iter().map(|v| (&v.kind, v.path.stem())).collect();
        assert_eq!(
            kinds,
            [
                (&ModelVariantKind::Dead, "GGM058_280mm54_5_C34_dead"),
                (&ModelVariantKind::Skin, "GGM546_280mm54_5_C34_Black")
            ]
        );

        let skin_entry = catalog.entry(&path(skin)).expect("skin entry");
        assert_eq!(skin_entry.skin_of(), [path(stock)]);
        assert_eq!(skin_entry.variants().len(), 1, "the skin keeps its own dead model");
    }

    #[test]
    fn buildings_group_by_the_kind_of_the_params_that_use_them() {
        let fort = "content/location/gameplay/building/LYB041/LYB041.model";
        let tower = "content/location/gameplay/building/LYB048_PA_Building/LYB048_PA_Building.model";
        let tower_wreck = "content/location/gameplay/building/LYB048_PA_Building_dead/LYB048_PA_Building_dead.model";
        let tower_wreck_part =
            "content/location/gameplay/building/LYB048_PA_Building_dead/LYB048_PA_Building_dead_Top.model";
        let base = "content/location/gameplay/building/LYB031/LYB031.model";
        let star_base = "content/location/gameplay/building/LYB205_StarTrek_01/LYB205_StarTrek_01.model";
        let field_gun = "content/location/gameplay/gun/main/LYGM002_105mm_LeFH_18/LYGM002_105mm_LeFH_18.model";
        let field_gun_dead =
            "content/location/gameplay/gun/main/LYGM002_105mm_LeFH_18/LYGM002_105mm_LeFH_18_dead.model";
        let ship_gun = "content/gameplay/germany/gun/main/GGM058_280mm54_5_C34/GGM058_280mm54_5_C34.model";
        let airfield = "content/location/nature/gameplay/LNG001/LNG001.model";
        let airfield_dead = "content/location/nature/gameplay/LNG001/LNG001_dead.model";
        let buildings = vec![
            building("PCBC022_PCGM002", BuildingKind::CoastalArtillery, fort, "", &[field_gun, field_gun_dead]),
            building("PCBX022_PCGM002", BuildingKind::Complex, fort, "", &[ship_gun]),
            building("PCBX023_PCGM003", BuildingKind::Complex, fort, "", &[]),
            // The wreck has its own directory and becomes the tower's dead variant.
            building("PCBR004", BuildingKind::RayTower, tower, tower_wreck, &[]),
            // LYB031 is a wreck here and a hull below, so it stays listed.
            building("PCBM016_STK_1", BuildingKind::Military, star_base, base, &[]),
            building("PCBX008", BuildingKind::Complex, base, base, &[]),
            building("PCBB001", BuildingKind::AirBase, airfield, airfield_dead, &[]),
            // A param without models links nothing.
            building("PCBF099_Airfield", BuildingKind::Unrecognized("Fake".to_string()), "", "", &[]),
            // A param without a species does not outvote the ones with a species.
            building("PCBZ001", BuildingKind::Unspecified, fort, "", &[]),
            building("PCBZ002", BuildingKind::Unspecified, fort, "", &[]),
            building("PCBZ003", BuildingKind::Unspecified, fort, "", &[]),
        ];
        let paths = [
            fort,
            tower,
            tower_wreck,
            tower_wreck_part,
            base,
            star_base,
            field_gun,
            field_gun_dead,
            ship_gun,
            airfield,
            airfield_dead,
        ];
        let catalog =
            ModelCatalog::from_paths(paths.iter().map(|p| path(p)), &CatalogLinks::new(Vec::new(), buildings));

        let rows: Vec<String> = catalog
            .entries()
            .iter()
            .map(|e| {
                let params: Vec<&str> = e.building_params().iter().map(BuildingParamRef::name).collect();
                let variants: Vec<&str> = e.variants().iter().map(|v| v.path.stem()).collect();
                format!("{} | {:?} | params {params:?} | variants {variants:?}", e.name(), e.group())
            })
            .collect();
        insta::assert_debug_snapshot!(rows);
        assert_eq!(catalog.owning_entry(&path(tower_wreck)).map(ModelEntry::name), Some("LYB048_PA_Building"));
        assert_eq!(
            catalog.owning_entry(&path(tower_wreck_part)).map(ModelEntry::name),
            Some("LYB048_PA_Building"),
            "a variant of the folded wreck moves with it"
        );
    }

    #[test]
    fn building_kinds_name_their_game_strings() {
        let complex = BuildingKind::from_species(&Recognized::Known(Species::Complex));
        assert_eq!(complex, BuildingKind::Complex);
        assert_eq!(complex.translation_key(), Some(TranslationKey::new("IDS_BUILDING_COMPLEX")));
        assert_eq!(
            BuildingKind::from_species(&Recognized::Unknown("Lighthouse".to_string())).translation_key(),
            Some(TranslationKey::new("IDS_BUILDING_LIGHTHOUSE"))
        );
        assert_eq!(BuildingKind::Weapon.translation_key(), None);
    }

    #[test]
    fn misc_names_classify_by_the_first_matching_keyword_row() {
        let stems = [
            "CM110_Propeller4_L",
            "RM869_Rudder_Indicator",
            "AM116_Boat_Winch_2",
            "ZM519_Longboat_34ft",
            "AM6242_Motor_boat_35ft_Black",
            "ZM971_Anchor_Capstan_high",
            "RM296_Cran_of_Boats",
            "WM864_Running_Lights_R",
            "AM1023_Code_Signal_Flags",
            "GM179_Crest_Mecklenburg",
            "WM5136_Leer_Skoda_1",
            "AM601_Skylight_Hatch",
            "ZM818_Ventilation_Mushroom",
            "WM5048_Radar_type_285",
            "AM6059_Compass_Binnacle",
            "JM495_Gun_Deck_Rangefinder",
            "WM5234_Gun_Deck_L",
            "GM041_52mm_SK_L55",
            "XM085_Wood_Box_2_Ice",
            "XM289_YHAR_FranceArc_M_1",
            "RM884",
        ];

        let categories: Vec<String> =
            stems.iter().map(|stem| format!("{stem} | {:?}", MiscCategory::classify(stem))).collect();
        insta::assert_debug_snapshot!(categories);
    }

    #[test]
    fn a_base_model_is_as_available_as_its_least_available_section() {
        let hull = "content/gameplay/usa/ship/battleship/ASB028_Iowa_1945/ASB028_Iowa_1945.model";
        let bow = "content/gameplay/usa/ship/battleship/ASB028_Iowa_1945/ASB028_Iowa_1945_Bow.model";
        let stern = "content/gameplay/usa/ship/battleship/ASB028_Iowa_1945/ASB028_Iowa_1945_Stern.model";
        let house = "content/location/building/city/LBC176/LBC176.model";
        let unknown = "content/location/building/city/LBC177/LBC177.model";
        let cruiser = "content/gameplay/usa/ship/cruiser/ASC011_Helena_1944/ASC011_Helena_1944.model";
        let cruiser_bow = "content/gameplay/usa/ship/cruiser/ASC011_Helena_1944/ASC011_Helena_1944_Bow.model";
        let geometry = HashMap::from([
            (path(hull), GeometryAvailability::Present),
            (path(bow), GeometryAvailability::Present),
            (path(stern), GeometryAvailability::Absent),
            (path(house), GeometryAvailability::NoGeometry),
            (path(cruiser), GeometryAvailability::Present),
        ]);
        let catalog = catalog(&[hull, bow, stern, house, unknown, cruiser, cruiser_bow], &[]).with_geometry(geometry);

        assert_eq!(catalog.geometry(&path(hull)), Some(GeometryAvailability::Absent), "the stern is absent");
        assert_eq!(catalog.geometry(&path(bow)), Some(GeometryAvailability::Present), "a section stands alone");
        assert_eq!(catalog.geometry(&path(house)), Some(GeometryAvailability::NoGeometry));
        assert_eq!(catalog.geometry(&path(unknown)), None);
        assert_eq!(catalog.geometry(&path(cruiser)), None, "a section of unknown availability");
    }
}
