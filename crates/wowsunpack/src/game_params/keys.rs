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
/// Names the upgrade this one follows in its slot's research chain; the chain root
/// (stock module) has an empty `prev`.
pub const PREV: &str = "prev";

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
pub const UC_TYPE_SUO: &str = "_Suo";

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
pub const COMP_FIRE_CONTROL: &str = "fireControl";
pub const COMP_INNATE_SKILLS: &str = "innateSkills";

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
// Visibility detection-range coefficients read by FactoryVisibility.createVisibilityTTX.
pub const VISIBILITY_COEF_FIRE: &str = "visibilityCoefFire";
pub const VISIBILITY_COEF_FIRE_BY_PLANE: &str = "visibilityCoefFireByPlane";
pub const VISIBILITY_COEF_GK: &str = "visibilityCoefGK";
pub const VISIBILITY_COEF_GK_IN_SMOKE: &str = "visibilityCoefGKInSmoke";
// Submarine per-depth visibility dict; PreprocessedHull.py:13 reads ['PERISCOPE'].
pub const VISIBILITY_FACTORS_BY_SUBMARINE: &str = "visibilityFactorsBySubmarine";
pub const VISIBILITY_PERISCOPE: &str = "PERISCOPE";
pub const MAX_DIST: &str = "maxDist";
// Fire-control range coefficient (PreprocessedFireControl.py:7; stock 1.0).
pub const MAX_DIST_COEF: &str = "maxDistCoef";
pub const AMMO_LIST: &str = "ammoList";
pub const SHOT_DELAY: &str = "shotDelay";
pub const ROTATION_SPEED: &str = "rotationSpeed";
pub const NUM_BARRELS: &str = "numBarrels";
pub const AMMO_SWITCH_COEFF: &str = "ammoSwitchCoeff";
pub const BARREL_DIAMETER: &str = "barrelDiameter";
pub const MIN_RADIUS: &str = "minRadius";
pub const IDEAL_RADIUS: &str = "idealRadius";
pub const IDEAL_DISTANCE: &str = "idealDistance";
pub const RADIUS_ON_ZERO: &str = "radiusOnZero";
pub const RADIUS_ON_DELIM: &str = "radiusOnDelim";
pub const RADIUS_ON_MAX: &str = "radiusOnMax";
pub const DISPERSION_DELIM: &str = "delim";
// Weapon hardpoint sub-object keys are `HP_<nation><kind>_<index>`, where
// `<nation>` is a single nation letter (A=USA, J=Japan, G=Germany, ...) and
// `<kind>` is GM (main battery), GS (secondary/ATBA), or GT (torpedoes). The
// nation letter varies per ship, so matching a single nation's literal prefix
// (e.g. HP_AGM) silently drops every other nation's mounts. Match the kind
// suffix nation-agnostically instead.
fn hardpoint_kind_matches(key: &str, kind: &str) -> bool {
    let Some(rest) = key.strip_prefix(HP_PREFIX) else {
        return false;
    };
    // One nation char, then the two-letter kind, then `_<index>`.
    let mut chars = rest.chars();
    chars.next().is_some() && chars.as_str().starts_with(kind) && rest[1 + kind.len()..].starts_with('_')
}

/// Main-battery gun sub-object key (HP_AGM_1, HP_JGM_1, HP_GGM_1, ...).
pub fn is_main_gun_hardpoint(key: &str) -> bool {
    hardpoint_kind_matches(key, "GM")
}

/// Secondary-battery (ATBA) gun sub-object key (HP_AGS_1, HP_JGS_1, HP_GGS_1, ...).
pub fn is_secondary_gun_hardpoint(key: &str) -> bool {
    hardpoint_kind_matches(key, "GS")
}

/// Torpedo launcher gun sub-object key (HP_AGT_1, HP_JGT_1, HP_GGT_1, ...).
pub fn is_torpedo_hardpoint(key: &str) -> bool {
    hardpoint_kind_matches(key, "GT")
}
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hardpoint_matchers_are_nation_agnostic() {
        // Each nation letter (A=USA, J=Japan, G=Germany) keys the same kind.
        for nation in ['A', 'J', 'G', 'F', 'I', 'R', 'U', 'B'] {
            assert!(is_main_gun_hardpoint(&format!("HP_{nation}GM_1")), "GM {nation}");
            assert!(is_secondary_gun_hardpoint(&format!("HP_{nation}GS_3")), "GS {nation}");
            assert!(is_torpedo_hardpoint(&format!("HP_{nation}GT_2")), "GT {nation}");
        }
    }

    #[test]
    fn hardpoint_matchers_reject_other_kinds_and_shapes() {
        // A main-gun matcher must not catch secondary/torpedo or aux mounts.
        assert!(!is_main_gun_hardpoint("HP_AGS_1"));
        assert!(!is_main_gun_hardpoint("HP_AGT_1"));
        assert!(!is_secondary_gun_hardpoint("HP_AGM_1"));
        assert!(!is_torpedo_hardpoint("HP_AGM_1"));
        // HP_RGA is a non-weapon mount: matches none of the three kinds.
        assert!(!is_main_gun_hardpoint("HP_RGA_1"));
        assert!(!is_secondary_gun_hardpoint("HP_RGA_1"));
        // Missing index separator, missing prefix, or extra nation chars are rejected.
        assert!(!is_main_gun_hardpoint("HP_AGM1"));
        assert!(!is_main_gun_hardpoint("AGM_1"));
        assert!(!is_main_gun_hardpoint("HP_AAGM_1"));
        assert!(!is_main_gun_hardpoint("HP_GM_1"));
    }
}
