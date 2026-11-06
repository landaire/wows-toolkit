//! A collision material of a ship's armor model, read as the zone of the ship
//! it belongs to and the part of that zone it is.
//!
//! The game names a plate by its material key with `IDS_<KEY>`, or with
//! `IDS_AC_<KEY>`, `IDS_DD_<KEY>` and `IDS_SUB_<KEY>` on carriers, destroyers
//! and submarines. Those names give the part only ("Deck", "Armor Belt") and
//! leave the zone to the armor viewer's grouping.

use std::fmt;

use crate::data::ResourceLoader;
use crate::data::TranslationKey;
use crate::game_params::ttx::armor_materials::COLLISION_MATERIAL_NAMES;
#[cfg(feature = "serde")]
use crate::game_params::ttx::armor_materials::UNKNOWN_MATERIAL_NAME;
use crate::game_params::ttx::armor_materials::collision_material_name;
use crate::game_params::types::Species;

/// One collision material, by its key in the material table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ArmorMaterial {
    name: &'static str,
    zone: MaterialZone,

    /// The second zone of a `Dual_*` material that separates two zones.
    shared_with: Option<MaterialZone>,
    part: MaterialPart,
}

impl ArmorMaterial {
    /// The material with table id `id`. An id beyond the table reads as
    /// [`MaterialZone::Unknown`].
    pub fn from_id(id: u8) -> Self {
        Self::from_table_name(collision_material_name(id))
    }

    /// The material keyed `name`, or `None` when the table has no such key.
    pub fn parse(name: &str) -> Option<Self> {
        COLLISION_MATERIAL_NAMES.iter().copied().find(|known| *known == name).map(Self::from_table_name)
    }

    fn from_table_name(name: &'static str) -> Self {
        let (zone, shared_with, part) = read_name(name);

        Self { name, zone, shared_with, part }
    }

    /// The material key, e.g. "Bow_ConstrSide".
    pub fn name(self) -> &'static str {
        self.name
    }

    pub fn zone(self) -> MaterialZone {
        self.zone
    }

    pub fn shared_with(self) -> Option<MaterialZone> {
        self.shared_with
    }

    pub fn part(self) -> MaterialPart {
        self.part
    }

    /// Plain English name with the zone, e.g. "bow side plating" or
    /// "citadel/casemate shared torpedo bulkhead".
    pub fn english_label(self) -> String {
        match (self.zone, self.shared_with) {
            (MaterialZone::Unassigned | MaterialZone::Unknown, _) => self.zone.to_string(),
            (zone, Some(other)) => format!("{zone}/{other} shared {}", self.part),
            (zone, None) => format!("{zone} {}", self.part),
        }
    }

    /// Translation keys the game names this material by on a ship of
    /// `species`, most specific first.
    pub fn translation_keys(self, species: Option<Species>) -> Vec<TranslationKey> {
        let key = self.name.to_uppercase();
        let class_prefix = match species {
            Some(Species::AirCarrier) => Some("AC"),
            Some(Species::Destroyer) => Some("DD"),
            Some(Species::Submarine) => Some("SUB"),
            _ => None,
        };

        let mut keys = Vec::with_capacity(2);
        if let Some(prefix) = class_prefix {
            keys.push(TranslationKey::new(format!("IDS_{prefix}_{key}")));
        }
        keys.push(TranslationKey::new(format!("IDS_{key}")));

        keys
    }

    /// The game's name for this material on a ship of `species`, or
    /// [`Self::english_label`] when the game has none.
    pub fn localized_name(self, loader: &dyn ResourceLoader, species: Option<Species>) -> String {
        self.game_name(loader, species).unwrap_or_else(|| self.english_label())
    }

    /// [`Self::localized_name`] led by the zone, for readers that see the
    /// name without the zone beside it: "bow: Fore End Plating".
    pub fn qualified_name(self, loader: &dyn ResourceLoader, species: Option<Species>) -> String {
        let Some(name) = self.game_name(loader, species) else {
            return self.english_label();
        };

        match self.shared_with {
            Some(other) => format!("{}/{other}: {name}", self.zone),
            None => format!("{}: {name}", self.zone),
        }
    }

    fn game_name(self, loader: &dyn ResourceLoader, species: Option<Species>) -> Option<String> {
        if matches!(self.zone, MaterialZone::Unassigned | MaterialZone::Unknown) {
            return None;
        }

        self.translation_keys(species).iter().find_map(|key| loader.localized_name_from_id(key))
    }
}

impl PartialOrd for ArmorMaterial {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// By key: the key determines every other field.
impl Ord for ArmorMaterial {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.name.cmp(other.name)
    }
}

#[cfg(feature = "serde")]
impl serde::Serialize for ArmorMaterial {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.name)
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for ArmorMaterial {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let name = <std::borrow::Cow<'de, str>>::deserialize(deserializer)?;
        if name == UNKNOWN_MATERIAL_NAME {
            return Ok(Self::from_table_name(UNKNOWN_MATERIAL_NAME));
        }

        Self::parse(&name).ok_or_else(|| serde::de::Error::custom(format_args!("unknown collision material {name:?}")))
    }
}

/// Main battery turret number in `Tur<N>Gk*` material keys, from 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TurretNumber(u8);

impl TurretNumber {
    pub fn new(number: u8) -> Self {
        Self(number)
    }

    pub fn get(self) -> u8 {
        self.0
    }
}

/// The region of the ship a material belongs to, from its key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MaterialZone {
    Bow,
    Stern,
    Citadel,
    /// `OCit`.
    OuterCitadel,
    Casemate,
    /// `SSC`: the game's auxiliary rooms; a carrier's hangar.
    AuxiliaryRooms,
    /// `SS`.
    Superstructure,
    /// Keys without a zone prefix.
    Hull,
    /// Main battery turret, numbered on barbette keys.
    Turret(Option<TurretNumber>),
    /// `AuTurret*` and `SG*`.
    SecondaryTurret,
    /// `Art*`.
    Magazine,
    /// `Bridge*` and `SS_Bridge*`.
    ConningTower,
    /// `Kdp*`.
    FireControlDirector,
    Funnel,
    /// `Rudder*`.
    SteeringGear,
    /// The bare `Bulge` key.
    TorpedoProtection,
    /// `common` and `zero`, which carry no armor type.
    Unassigned,
    /// A key this module does not read, or an id beyond the table.
    Unknown,
}

impl fmt::Display for MaterialZone {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let words = match self {
            Self::Bow => "bow",
            Self::Stern => "stern",
            Self::Citadel => "citadel",
            Self::OuterCitadel => "outer citadel",
            Self::Casemate => "casemate",
            Self::AuxiliaryRooms => "auxiliary room",
            Self::Superstructure => "superstructure",
            Self::Hull => "hull",
            Self::Turret(Some(number)) => return write!(f, "turret {}", number.get()),
            Self::Turret(None) => "turret",
            Self::SecondaryTurret => "secondary turret",
            Self::Magazine => "magazine",
            Self::ConningTower => "conning tower",
            Self::FireControlDirector => "fire control director",
            Self::Funnel => "funnel",
            Self::SteeringGear => "steering gear",
            Self::TorpedoProtection => "torpedo protection",
            Self::Unassigned => "unassigned material",
            Self::Unknown => "unknown material",
        };
        f.write_str(words)
    }
}

/// The part of its zone a material is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MaterialPart {
    Belt,
    Side,
    /// `ConstrSide`: unarmored or lightly armored hull side.
    SidePlating,
    Deck,
    /// `Inclin`.
    DeckSlope,
    Bottom,
    /// `Trans`.
    TransverseBulkhead,
    ForwardTransverseBulkhead,
    AftTransverseBulkhead,
    /// `Bulge` within a zone.
    TorpedoBulkhead,
    Barbette,
    /// `GkDown` and `SGDown`.
    LowerBarbette,
    /// `GkTop`.
    BarbetteHatch,
    /// `Top`.
    Roof,
    Front,
    Rear,
    FlightDeck,
    /// `Hang`.
    HangarPlating,
    Plating,
    /// `ArtSide` on a `Dual_*` key.
    MagazineSide,
    /// `ArtDeck` on a `Dual_*` key.
    MagazineDeck,
    Unassigned,
    Unknown,
}

impl fmt::Display for MaterialPart {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Belt => "belt",
            Self::Side => "side",
            Self::SidePlating => "side plating",
            Self::Deck => "deck",
            Self::DeckSlope => "deck slope",
            Self::Bottom => "bottom",
            Self::TransverseBulkhead => "transverse bulkhead",
            Self::ForwardTransverseBulkhead => "forward transverse bulkhead",
            Self::AftTransverseBulkhead => "aft transverse bulkhead",
            Self::TorpedoBulkhead => "torpedo bulkhead",
            Self::Barbette => "barbette",
            Self::LowerBarbette => "lower barbette",
            Self::BarbetteHatch => "barbette hatch",
            Self::Roof => "roof",
            Self::Front => "front",
            Self::Rear => "rear",
            Self::FlightDeck => "flight deck",
            Self::HangarPlating => "hangar plating",
            Self::Plating => "plating",
            Self::MagazineSide => "magazine side",
            Self::MagazineDeck => "magazine deck",
            Self::Unassigned => "unassigned",
            Self::Unknown => "unknown",
        })
    }
}

/// Zone, second zone of a `Dual_*` key, and part of the material keyed `name`.
fn read_name(name: &str) -> (MaterialZone, Option<MaterialZone>, MaterialPart) {
    const UNKNOWN: (MaterialZone, Option<MaterialZone>, MaterialPart) =
        (MaterialZone::Unknown, None, MaterialPart::Unknown);

    if let Some(rest) = name.strip_prefix("Dual_") {
        let mut tokens = rest.splitn(3, '_');
        let (Some(first), Some(second), Some(part)) = (tokens.next(), tokens.next(), tokens.next()) else {
            return UNKNOWN;
        };
        let (Some(zone), Some(other), Some(part)) = (prefix_zone(first), prefix_zone(second), dual_part(part)) else {
            return UNKNOWN;
        };

        return (zone, (other != zone).then_some(other), part);
    }

    match single_zone_part(name) {
        Some((zone, part)) => (zone, None, part),
        None => UNKNOWN,
    }
}

fn single_zone_part(name: &str) -> Option<(MaterialZone, MaterialPart)> {
    use MaterialPart as Part;
    use MaterialZone as Zone;

    match name {
        "common" | "zero" => return Some((Zone::Unassigned, Part::Unassigned)),
        "SSCasemate" => return Some((Zone::AuxiliaryRooms, Part::Plating)),
        "Bulge" => return Some((Zone::TorpedoProtection, Part::Plating)),
        "SGBarbetteSS" | "SS_SGBarbette" => return Some((Zone::SecondaryTurret, Part::Barbette)),
        "SGDownSS" | "SS_SGDown" => return Some((Zone::SecondaryTurret, Part::LowerBarbette)),
        _ => {}
    }

    if let Some(found) = turret_barbette(name) {
        return Some(found);
    }
    if let Some(rest) = name.strip_prefix("Rudder") {
        let part = match rest {
            "Aft" => Part::AftTransverseBulkhead,
            "Fwd" => Part::ForwardTransverseBulkhead,
            "Side" => Part::Side,
            "Top" => Part::Deck,
            _ => return None,
        };
        return Some((Zone::SteeringGear, part));
    }

    let fittings = [
        ("SS_Bridge", Zone::ConningTower),
        ("AuTurret", Zone::SecondaryTurret),
        ("Turret", Zone::Turret(None)),
        ("Bridge", Zone::ConningTower),
        ("Kdp", Zone::FireControlDirector),
        ("Funnel", Zone::Funnel),
        ("Art", Zone::Magazine),
    ];
    for (prefix, zone) in fittings {
        if let Some(rest) = name.strip_prefix(prefix) {
            return fitting_part(rest, zone).map(|part| (zone, part));
        }
    }

    if let Some((prefix, rest)) = name.split_once('_') {
        return Some((prefix_zone(prefix)?, zone_part(rest)?));
    }

    // Faces named part first: "SideCit", "DeckStern".
    let faces = [
        ("Cit", Zone::Citadel),
        ("Cas", Zone::Casemate),
        ("SSC", Zone::AuxiliaryRooms),
        ("Bow", Zone::Bow),
        ("Stern", Zone::Stern),
        ("SS", Zone::Superstructure),
    ];
    for (suffix, zone) in faces {
        if let Some(face) = name.strip_suffix(suffix)
            && let Some(part) = face_part(face)
        {
            return Some((zone, part));
        }
    }

    zone_part(name).map(|part| (Zone::Hull, part))
}

/// `Tur<N>GkBar`, `Tur<N>GkDown` and `Tur<N>GkTop`.
fn turret_barbette(name: &str) -> Option<(MaterialZone, MaterialPart)> {
    let rest = name.strip_prefix("Tur")?;
    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
    let number: u8 = rest[..digits].parse().ok()?;
    let part = match &rest[digits..] {
        "GkBar" => MaterialPart::Barbette,
        "GkDown" => MaterialPart::LowerBarbette,
        "GkTop" => MaterialPart::BarbetteHatch,
        _ => return None,
    };

    Some((MaterialZone::Turret(Some(TurretNumber::new(number))), part))
}

fn prefix_zone(prefix: &str) -> Option<MaterialZone> {
    Some(match prefix {
        "Bow" => MaterialZone::Bow,
        "St" => MaterialZone::Stern,
        "Cit" => MaterialZone::Citadel,
        "OCit" => MaterialZone::OuterCitadel,
        "Cas" => MaterialZone::Casemate,
        "SSC" => MaterialZone::AuxiliaryRooms,
        "SS" => MaterialZone::Superstructure,
        _ => return None,
    })
}

/// The part after a zone prefix, or of a key with no prefix.
fn zone_part(part: &str) -> Option<MaterialPart> {
    Some(match part {
        "Belt" => MaterialPart::Belt,
        "Side" => MaterialPart::Side,
        "ConstrSide" => MaterialPart::SidePlating,
        "Deck" => MaterialPart::Deck,
        "Inclin" => MaterialPart::DeckSlope,
        "Bottom" => MaterialPart::Bottom,
        "Trans" => MaterialPart::TransverseBulkhead,
        "FwdTrans" => MaterialPart::ForwardTransverseBulkhead,
        "AftTrans" => MaterialPart::AftTransverseBulkhead,
        "Bulge" => MaterialPart::TorpedoBulkhead,
        "Barbette" => MaterialPart::Barbette,
        "Top" => MaterialPart::Roof,
        "Fdck" => MaterialPart::FlightDeck,
        "Hang" => MaterialPart::HangarPlating,
        _ => return None,
    })
}

fn dual_part(part: &str) -> Option<MaterialPart> {
    match part {
        "ArtSide" => Some(MaterialPart::MagazineSide),
        "ArtDeck" => Some(MaterialPart::MagazineDeck),
        _ => zone_part(part),
    }
}

/// The part of a key named part first, before its zone.
fn face_part(face: &str) -> Option<MaterialPart> {
    Some(match face {
        "Side" => MaterialPart::Side,
        "Deck" => MaterialPart::Deck,
        "Trans" => MaterialPart::TransverseBulkhead,
        "Inclin" => MaterialPart::DeckSlope,
        _ => return None,
    })
}

/// The part of a turret, conning tower, director, funnel or magazine key.
fn fitting_part(part: &str, zone: MaterialZone) -> Option<MaterialPart> {
    Some(match part {
        "Side" => MaterialPart::Side,
        // The game names a magazine top "Deck".
        "Top" if zone == MaterialZone::Magazine => MaterialPart::Deck,
        "Top" => MaterialPart::Roof,
        "Front" | "Fwd" => MaterialPart::Front,
        "Aft" => MaterialPart::Rear,
        "Bottom" | "Down" => MaterialPart::Bottom,
        "Barbette" | "Barbette2" => MaterialPart::Barbette,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game_params::types::Param;
    use crate::game_types::GameParamId;
    use crate::rpc::entitydefs::EntitySpec;

    /// Knows the given keys only.
    struct Catalog(&'static [(&'static str, &'static str)]);

    impl ResourceLoader for Catalog {
        fn localized_name_from_param(&self, _param: &Param) -> Option<String> {
            None
        }

        fn localized_name_from_id(&self, id: &TranslationKey) -> Option<String> {
            self.0.iter().find(|(key, _)| *key == id.as_str()).map(|(_, name)| (*name).to_owned())
        }

        fn game_param_by_id(&self, _id: GameParamId) -> Option<crate::Rc<Param>> {
            None
        }

        fn entity_specs(&self) -> &[EntitySpec] {
            &[]
        }
    }

    fn material(name: &str) -> ArmorMaterial {
        ArmorMaterial::parse(name).expect("key is in the table")
    }

    #[test]
    fn every_table_key_is_read() {
        let unread: Vec<&str> = COLLISION_MATERIAL_NAMES
            .iter()
            .filter(|name| matches!(material(name).zone(), MaterialZone::Unknown))
            .copied()
            .collect();

        assert_eq!(unread, Vec::<&str>::new());
    }

    #[test]
    fn english_labels_of_every_key() {
        let labels: Vec<String> =
            COLLISION_MATERIAL_NAMES.iter().map(|name| format!("{name}: {}", material(name).english_label())).collect();

        insta::assert_yaml_snapshot!(labels);
    }

    #[test]
    fn ids_beyond_the_table_are_unknown() {
        let beyond = ArmorMaterial::from_id(255);

        assert_eq!(beyond.zone(), MaterialZone::Unknown);
        assert_eq!(beyond.english_label(), "unknown material");
    }

    #[test]
    fn class_name_comes_before_the_general_one() {
        const CATALOG: Catalog =
            Catalog(&[("IDS_AC_SSC_CONSTRSIDE", "Hangar Plating"), ("IDS_SSC_CONSTRSIDE", "Plating")]);
        let side = material("SSC_ConstrSide");

        assert_eq!(side.localized_name(&CATALOG, Some(Species::AirCarrier)), "Hangar Plating");
        assert_eq!(side.localized_name(&CATALOG, Some(Species::Cruiser)), "Plating");
        assert_eq!(side.qualified_name(&CATALOG, Some(Species::Cruiser)), "auxiliary room: Plating");
    }

    #[test]
    fn unnamed_material_falls_back_to_the_english_label() {
        let shared = material("Dual_Cit_Cas_Bulge");

        assert_eq!(shared.localized_name(&Catalog(&[]), None), "citadel/casemate shared torpedo bulkhead");
        assert_eq!(
            shared.qualified_name(&Catalog(&[("IDS_DUAL_CIT_CAS_BULGE", "Torpedo Bulkhead")]), None),
            "citadel/casemate: Torpedo Bulkhead"
        );
    }

    #[cfg(feature = "serde")]
    #[test]
    fn serializes_as_its_key() {
        let json = serde_json::to_string(&material("Bow_Deck")).expect("serializes");

        assert_eq!(json, "\"Bow_Deck\"");
        assert_eq!(serde_json::from_str::<ArmorMaterial>(&json).expect("deserializes"), material("Bow_Deck"));
    }

    #[cfg(feature = "serde")]
    #[test]
    fn an_id_beyond_the_table_round_trips() {
        let beyond = ArmorMaterial::from_id(255);
        let json = serde_json::to_string(&beyond).expect("serializes");

        assert_eq!(serde_json::from_str::<ArmorMaterial>(&json).expect("deserializes"), beyond);
    }
}
