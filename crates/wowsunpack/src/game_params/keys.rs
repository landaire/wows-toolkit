//! Constants for GameParams pickle dictionary keys.
//!
//! These replace the hardcoded `HashableValue::String("...".to_string().into())`
//! patterns scattered throughout `provider.rs` and `main.rs`.

// Ship top-level keys
pub const SHIP_UPGRADE_INFO: &str = "ShipUpgradeInfo";
pub const SHIP_ABILITIES: &str = "ShipAbilities";
pub const A_HULL: &str = "A_Hull";

// Upgrade dict keys
pub const UC_TYPE: &str = "ucType";
pub const COMPONENTS: &str = "components";

// ucType values
pub const UC_TYPE_ENGINE: &str = "_Engine";
pub const UC_TYPE_HULL: &str = "_Hull";
pub const UC_TYPE_ARTILLERY: &str = "_Artillery";
pub const UC_TYPE_TORPEDOES: &str = "_Torpedoes";
pub const UC_TYPE_ATBA: &str = "_ATBA";
pub const UC_TYPE_AIR_DEFENSE: &str = "_AirDefense";
pub const UC_TYPE_DIRECTORS: &str = "_Directors";
pub const UC_TYPE_FINDERS: &str = "_Finders";
pub const UC_TYPE_RADARS: &str = "_Radars";

// Component type keys (inside "components" dict)
pub const COMP_HULL: &str = "hull";
pub const COMP_ARTILLERY: &str = "artillery";
pub const COMP_ATBA: &str = "atba";
pub const COMP_AIR_DEFENSE: &str = "airDefense";
pub const COMP_DIRECTORS: &str = "directors";
pub const COMP_FINDERS: &str = "finders";
pub const COMP_RADARS: &str = "radars";
pub const COMP_TORPEDOES: &str = "torpedoes";
pub const COMP_ENGINE: &str = "engine";

/// Typed representation of component type keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
#[cfg_attr(feature = "rkyv", rkyv(derive(Hash, PartialEq, Eq)))]
pub enum ComponentType {
    #[cfg_attr(feature = "serde", serde(rename = "hull"))]
    Hull,
    #[cfg_attr(feature = "serde", serde(rename = "artillery"))]
    Artillery,
    #[cfg_attr(feature = "serde", serde(rename = "atba"))]
    Atba,
    #[cfg_attr(feature = "serde", serde(rename = "airDefense"))]
    AirDefense,
    #[cfg_attr(feature = "serde", serde(rename = "directors"))]
    Directors,
    #[cfg_attr(feature = "serde", serde(rename = "finders"))]
    Finders,
    #[cfg_attr(feature = "serde", serde(rename = "radars"))]
    Radars,
    #[cfg_attr(feature = "serde", serde(rename = "torpedoes"))]
    Torpedoes,
}

impl ComponentType {
    /// All known component types.
    pub const ALL: &[ComponentType] = &[
        Self::Hull,
        Self::Artillery,
        Self::Atba,
        Self::AirDefense,
        Self::Directors,
        Self::Finders,
        Self::Radars,
        Self::Torpedoes,
    ];

    /// The raw string key used in GameParams dictionaries.
    pub fn key(&self) -> &'static str {
        match self {
            Self::Hull => "hull",
            Self::Artillery => "artillery",
            Self::Atba => "atba",
            Self::AirDefense => "airDefense",
            Self::Directors => "directors",
            Self::Finders => "finders",
            Self::Radars => "radars",
            Self::Torpedoes => "torpedoes",
        }
    }

    /// The raw `ucType` string a unit reports for this component type (e.g.
    /// `"_Hull"`, `"_Artillery"`). This is the value carried by a unit/module
    /// param's `ucType` field, distinct from the `components`-dict `key()`.
    /// Comparison is case-insensitive on the suffix because GameParams is not
    /// consistent in casing across versions (`_ATBA` vs `_Atba`).
    pub fn uc_type(&self) -> &'static str {
        match self {
            Self::Hull => UC_TYPE_HULL,
            Self::Artillery => UC_TYPE_ARTILLERY,
            Self::Atba => UC_TYPE_ATBA,
            Self::AirDefense => UC_TYPE_AIR_DEFENSE,
            Self::Directors => UC_TYPE_DIRECTORS,
            Self::Finders => UC_TYPE_FINDERS,
            Self::Radars => UC_TYPE_RADARS,
            Self::Torpedoes => UC_TYPE_TORPEDOES,
        }
    }

    /// Parse a raw unit `ucType` string (e.g. `"_Hull"`, `"_Artillery"`) into a
    /// `ComponentType`. Returns `None` for ucType values that have no matching
    /// component variant (e.g. `"_Suo"` fire control, `"_Engine"`), which is the
    /// caller's signal that the unit is not one of the modelled components. The
    /// match is case-insensitive to tolerate cross-version casing differences.
    pub fn from_uc_type(uc_type: &str) -> Option<ComponentType> {
        Self::ALL.iter().copied().find(|ct| ct.uc_type().eq_ignore_ascii_case(uc_type))
    }
}

impl std::fmt::Display for ComponentType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Hull => write!(f, "Hull"),
            Self::Artillery => write!(f, "Main Battery"),
            Self::Atba => write!(f, "Secondaries"),
            Self::AirDefense => write!(f, "AA"),
            Self::Directors => write!(f, "Directors"),
            Self::Finders => write!(f, "Finders"),
            Self::Radars => write!(f, "Radars"),
            Self::Torpedoes => write!(f, "Torpedoes"),
        }
    }
}

/// All component type keys.
pub const ALL_COMPONENT_TYPES: &[&str] = &[
    COMP_HULL,
    COMP_ARTILLERY,
    COMP_ATBA,
    COMP_AIR_DEFENSE,
    COMP_DIRECTORS,
    COMP_FINDERS,
    COMP_RADARS,
    COMP_TORPEDOES,
];

/// Component types that have 3D models (mounted on hull hardpoints).
pub const MODEL_COMPONENT_TYPES: &[&str] = &[
    COMP_HULL,
    COMP_ARTILLERY,
    COMP_ATBA,
    COMP_AIR_DEFENSE,
    COMP_DIRECTORS,
    COMP_FINDERS,
    COMP_RADARS,
    COMP_TORPEDOES,
];

// Data field keys
pub const MODEL: &str = "model";
pub const ARMOR: &str = "armor";
pub const HIT_LOCATION_GROUPS: &str = "hitLocationGroups";
pub const HL_TYPE: &str = "hlType";
pub const MAX_HP: &str = "maxHP";
pub const REGENERATED_HP_PART: &str = "regeneratedHPPart";
pub const SPLASH_BOXES: &str = "splashBoxes";
pub const THICKNESS: &str = "thickness";
pub const DRAFT: &str = "draft";
pub const DOCK_Y_OFFSET: &str = "dockYOffset";
pub const HEALTH: &str = "health";
pub const MAX_SPEED: &str = "maxSpeed";
pub const SPEED_COEF: &str = "speedCoef";
pub const TURNING_RADIUS: &str = "turningRadius";
pub const RUDDER_TIME: &str = "rudderTime";
pub const VISIBILITY_FACTOR: &str = "visibilityFactor";
pub const FLOOD_NODES: &str = "floodNodes";
pub const SUBMARINE_BATTERY: &str = "SubmarineBattery";
pub const BATTERY_CAPACITY: &str = "capacity";
pub const BATTERY_REGEN_RATE: &str = "regenRate";
pub const VISIBILITY_FACTOR_BY_PLANE: &str = "visibilityFactorByPlane";
pub const MAX_DIST: &str = "maxDist";
pub const AMMO_LIST: &str = "ammoList";
pub const SHOT_DELAY: &str = "shotDelay";
pub const ROTATION_SPEED: &str = "rotationSpeed";
pub const NUM_BARRELS: &str = "numBarrels";
pub const AMMO_SWITCH_COEFF: &str = "ammoSwitchCoeff";
// Torpedo launcher gun sub-object key prefix (HP_AGT_1, HP_AGT_2, ...).
pub const HP_AGT_PREFIX: &str = "HP_AGT";
pub const CAMOUFLAGE: &str = "camouflage";
pub const PERMOFLAGES: &str = "permoflages";
pub const TITLE: &str = "title";

// HP_ mount prefix
pub const HP_PREFIX: &str = "HP_";

// typeinfo keys
pub const TYPEINFO: &str = "typeinfo";
pub const TYPEINFO_TYPE: &str = "type";
pub const TYPEINFO_NATION: &str = "nation";
pub const TYPEINFO_SPECIES: &str = "species";

// Param identity keys
pub const PARAM_ID: &str = "id";
pub const PARAM_INDEX: &str = "index";
pub const PARAM_NAME: &str = "name";
