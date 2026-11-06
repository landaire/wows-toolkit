use std::collections::BTreeMap;
use std::collections::BTreeSet;

use serde::Serialize;
use serde::Serializer;
use wows_core::units::Millimeters;
use wowsunpack::export::gltf_export::zone_from_material_name;
use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::game_params::ttx::armor_material::ArmorMaterial;
use wowsunpack::game_params::ttx::armor_material::MaterialPart;
use wowsunpack::game_params::ttx::armor_material::MaterialZone;
use wowsunpack::game_params::ttx::armor_materials::armor_type_classifies;
use wowsunpack::game_params::types::Species;

use crate::data::GameData;
use crate::error::QueryError;
use crate::names::Interpretations;
use crate::ships::Ship;
use crate::ships::ShipArgs;
use crate::ships::ShipSummary;

#[derive(Debug, Clone, Serialize)]
pub struct ShipArmor {
    /// Names read as something they do not spell exactly.
    #[serde(skip_serializing_if = "Interpretations::is_empty")]
    pub interpretations: Interpretations,

    pub ship: ShipSummary,

    /// Plates shown in the port armor view, by hull zone.
    pub zones: Vec<ZoneArmor>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ZoneArmor {
    /// Hull zone as the WoWs Toolkit armor viewer names it.
    pub zone: &'static str,

    /// Plates above the waterline; absent when every plate of the zone is
    /// below it.
    #[serde(flatten)]
    pub above_water: Option<ThicknessSpan>,

    pub plates: Vec<PlateArmor>,
}

/// Thickness of a zone's plates above the waterline.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(untagged)]
pub enum ThicknessSpan {
    /// Every plate has this thickness.
    Uniform {
        thickness_mm: Millimeters,
    },
    Range {
        thinnest_mm: Millimeters,
        thickest_mm: Millimeters,
    },
}

impl ThicknessSpan {
    fn new(thinnest: Millimeters, thickest: Millimeters) -> Self {
        if thinnest == thickest {
            Self::Uniform { thickness_mm: thinnest }
        } else {
            Self::Range { thinnest_mm: thinnest, thickest_mm: thickest }
        }
    }

    pub fn thinnest(self) -> Millimeters {
        match self {
            Self::Uniform { thickness_mm } => thickness_mm,
            Self::Range { thinnest_mm, .. } => thinnest_mm,
        }
    }

    pub fn thickest(self) -> Millimeters {
        match self {
            Self::Uniform { thickness_mm } => thickness_mm,
            Self::Range { thickest_mm, .. } => thickest_mm,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PlateArmor {
    /// The game's name for the plate, led by its zone, e.g. "bow: Fore End Deck".
    pub plate: String,

    #[serde(skip)]
    pub material: ArmorMaterial,

    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub below_waterline: bool,

    /// One thickness, or each layer's ascending when the plate has several.
    #[serde(serialize_with = "one_or_list")]
    pub thickness_mm: Vec<Millimeters>,
}

/// The plate thickness a shell must defeat to damage a zone.
#[derive(Debug, Clone)]
pub(crate) struct ZoneThickness {
    pub(crate) thickness: Millimeters,

    /// Names of the zone's plates that have this thickness.
    pub(crate) plates: Vec<String>,
}

/// A plate of a ship's armor table that the armor view shows, with its
/// nonzero layers.
pub(crate) struct Plate {
    pub(crate) zone: &'static str,
    pub(crate) material: ArmorMaterial,

    /// Ascending and deduplicated.
    pub(crate) layers: Vec<Millimeters>,
}

impl Plate {
    fn is_above_water(&self) -> bool {
        is_above_water(self.material)
    }
}

impl GameData {
    /// Armor of one ship by zone and plate.
    pub fn ship_armor(&self, args: &ShipArgs) -> Result<ShipArmor, QueryError> {
        let mut interpretations = Interpretations::default();
        let ship = self.resolve_ship(&args.ship, &mut interpretations)?;
        let plates = plates(ship);
        let names = self.plate_names(ship, plates.iter().map(|plate| plate.material));
        let spans = zone_spans(&plates);

        let mut zones: Vec<ZoneArmor> = Vec::new();
        for plate in &plates {
            let named = PlateArmor {
                plate: names.of(plate.material),
                material: plate.material,
                below_waterline: !plate.is_above_water(),
                thickness_mm: plate.layers.clone(),
            };
            match zones.last_mut() {
                Some(zone) if zone.zone == plate.zone => zone.plates.push(named),
                _ => zones.push(ZoneArmor {
                    zone: plate.zone,
                    above_water: spans.get(plate.zone).copied(),
                    plates: vec![named],
                }),
            }
        }

        Ok(ShipArmor { interpretations, ship: ship.summary(), zones })
    }

    /// Names for the plates of `ship` that are made of `materials`.
    pub(crate) fn plate_names(
        &self,
        ship: &Ship,
        materials: impl IntoIterator<Item = ArmorMaterial>,
    ) -> PlateNames<'_> {
        let provider = self.provider();
        let species = ship.class.species();

        let materials: BTreeSet<ArmorMaterial> = materials.into_iter().collect();
        let mut by_name: BTreeMap<String, Vec<ArmorMaterial>> = BTreeMap::new();
        for material in materials {
            by_name.entry(material.qualified_name(provider, Some(species))).or_default().push(material);
        }

        let mut names: BTreeMap<ArmorMaterial, String> = BTreeMap::new();
        for (name, materials) in by_name {
            let shared = materials.len() > 1;
            for material in materials {
                let name = if shared { format!("{name} ({})", material.part()) } else { name.clone() };
                names.insert(material, name);
            }
        }

        PlateNames { provider, species, names }
    }
}

/// Names the plates of one ship. Plates the game names alike are told apart
/// by their part: "superstructure: Plating (roof)".
pub(crate) struct PlateNames<'a> {
    provider: &'a GameMetadataProvider,
    species: Species,
    names: BTreeMap<ArmorMaterial, String>,
}

impl PlateNames<'_> {
    /// The game's name for `material`, led by its zone.
    pub(crate) fn of(&self, material: ArmorMaterial) -> String {
        match self.names.get(&material) {
            Some(name) => name.clone(),
            // A material the names were not built from has no plate to be
            // told apart from.
            None => material.qualified_name(self.provider, Some(self.species)),
        }
    }
}

/// Per hull zone, the thinnest and thickest plate above the waterline.
pub(crate) fn zone_spans(plates: &[Plate]) -> BTreeMap<&'static str, ThicknessSpan> {
    let mut bounds: BTreeMap<&'static str, (Millimeters, Millimeters)> = BTreeMap::new();
    for plate in plates.iter().filter(|plate| plate.is_above_water()) {
        for thickness in plate.layers.iter().copied() {
            let entry = bounds.entry(plate.zone).or_insert((thickness, thickness));
            if thickness.value() < entry.0.value() {
                entry.0 = thickness;
            }
            if thickness.value() > entry.1.value() {
                entry.1 = thickness;
            }
        }
    }

    bounds.into_iter().map(|(zone, (thinnest, thickest))| (zone, ThicknessSpan::new(thinnest, thickest))).collect()
}

/// Plates of `ship` that the armor view classifies, sorted by zone and
/// material. Layers of 0 mm are left out: they close a zone's volume for hit
/// detection and stop nothing.
pub(crate) fn plates(ship: &Ship) -> Vec<Plate> {
    let Some(armor) = ship.param.vehicle().and_then(|vehicle| vehicle.armor()) else {
        return Vec::new();
    };

    let mut plates: Vec<Plate> = Vec::new();
    for (material, layers) in armor {
        let Ok(material) = u8::try_from(*material) else {
            continue;
        };
        let material = ArmorMaterial::from_id(material);
        if !armor_type_classifies(material.name()) {
            continue;
        }

        let mut layers: Vec<Millimeters> =
            layers.values().map(|mm| Millimeters::new(*mm)).filter(|mm| mm.value() > 0.0).collect();
        if layers.is_empty() {
            continue;
        }
        layers.sort_by(|left, right| left.value().total_cmp(&right.value()));
        layers.dedup();

        plates.push(Plate { zone: zone_from_material_name(material.name()), material, layers });
    }
    plates.sort_by(|left, right| left.zone.cmp(right.zone).then_with(|| left.material.cmp(&right.material)));

    plates
}

/// Thickness to test a shell against. A plate selects that plate's thickest
/// layer; it is named by its material key, its English label or its name as
/// ship_armor gives it. A zone name such as "Bow" selects the thinnest plate
/// of that zone above the waterline, which is the plate a shell must defeat
/// to damage the zone.
pub(crate) fn zone_thickness(game: &GameData, ship: &Ship, zone: &str) -> Result<ZoneThickness, QueryError> {
    let plates = plates(ship);
    let names = game.plate_names(ship, plates.iter().map(|plate| plate.material));
    let wanted = zone.trim();

    let named: Vec<&Plate> = plates
        .iter()
        .filter(|plate| {
            let material = plate.material;
            material.name().eq_ignore_ascii_case(wanted)
                || material.english_label().eq_ignore_ascii_case(wanted)
                || names.of(material).eq_ignore_ascii_case(wanted)
        })
        .collect();
    match named.as_slice() {
        [] => {}
        [plate] => {
            if let Some(thickest) = plate.layers.last() {
                return Ok(ZoneThickness { thickness: *thickest, plates: vec![names.of(plate.material)] });
            }
        }
        several => {
            return Err(QueryError::AmbiguousPlate {
                ship: ship.name.clone(),
                plate: wanted.to_owned(),
                candidates: several.iter().map(|plate| names.of(plate.material)).collect(),
            });
        }
    }

    let zone_name = canonical_zone(zone);
    let mut thinnest: Option<ZoneThickness> = None;
    for plate in plates.iter().filter(|plate| plate.zone.eq_ignore_ascii_case(zone_name) && plate.is_above_water()) {
        let Some(thickness) = plate.layers.first().copied() else {
            continue;
        };

        match &mut thinnest {
            Some(current) if current.thickness == thickness => current.plates.push(names.of(plate.material)),
            Some(current) if current.thickness.value() < thickness.value() => {}
            _ => thinnest = Some(ZoneThickness { thickness, plates: vec![names.of(plate.material)] }),
        }
    }

    thinnest.ok_or_else(|| {
        let mut zones: Vec<&'static str> = plates.iter().map(|plate| plate.zone).collect();
        zones.dedup();
        QueryError::ZoneNotFound { ship: ship.name.clone(), zone: zone.to_owned(), zones }
    })
}

/// Bottom plates of the hull and its zones lie below the waterline.
fn is_above_water(material: ArmorMaterial) -> bool {
    let hull_zone = matches!(
        material.zone(),
        MaterialZone::Bow | MaterialZone::Stern | MaterialZone::Citadel | MaterialZone::Casemate | MaterialZone::Hull
    );

    !(hull_zone && material.part() == MaterialPart::Bottom)
}

/// Maps player wording for a hull zone to the names `zone_from_material_name`
/// produces.
fn canonical_zone(zone: &str) -> &str {
    match zone.trim().to_lowercase().as_str() {
        "bow" | "nose" | "fore" | "front" => "Bow",
        "stern" | "st" | "aft" | "rear" => "Stern",
        "citadel" | "cit" => "Citadel",
        "casemate" | "cas" | "midsection" => "Casemate",
        "superstructure" | "ss" => "Superstructure",
        "turret" | "turrets" => "Turret",
        "steering gear" | "steering" | "sg" => "SteeringGear",
        "torpedo protection" | "tdz" | "bulge" => "TorpedoProtection",
        "hull" => "Hull",
        _ => zone.trim(),
    }
}

fn one_or_list<S: Serializer>(layers: &[Millimeters], serializer: S) -> Result<S::Ok, S::Error> {
    match layers {
        [single] => single.serialize(serializer),
        _ => layers.serialize(serializer),
    }
}
