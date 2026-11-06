use std::collections::BTreeMap;

use serde::Serialize;
use wows_core::units::Millimeters;
use wowsunpack::export::gltf_export::zone_from_material_name;
use wowsunpack::game_params::ttx::armor_materials::armor_type_classifies;
use wowsunpack::game_params::ttx::armor_materials::collision_material_name;

use crate::data::GameData;
use crate::error::QueryError;
use crate::ships::Ship;
use crate::ships::ShipArgs;
use crate::ships::ShipSummary;

#[derive(Debug, Clone, Serialize)]
pub struct ShipArmor {
    pub ship: ShipSummary,

    /// Per hull zone, the thinnest and thickest nonzero plate above the waterline.
    pub zones: Vec<ZoneArmor>,

    /// Plates shown in the port armor view, by collision material. Names ending
    /// in `_Bottom` are below the waterline.
    pub plates: Vec<PlateArmor>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ZoneArmor {
    /// Hull zone as the WoWs Toolkit armor viewer names it.
    pub zone: &'static str,
    pub min_mm: Millimeters,
    pub max_mm: Millimeters,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlateArmor {
    pub zone: &'static str,
    pub material: &'static str,

    /// Thickness of each layer, ascending and deduplicated.
    pub thickness_mm: Vec<Millimeters>,
}

/// The plate thickness a shell must defeat to damage a zone.
#[derive(Debug, Clone, Serialize)]
pub struct ZoneThickness {
    pub thickness: Millimeters,

    /// Plates of the zone that have this thickness.
    pub plates: Vec<&'static str>,
}

impl GameData {
    /// Armor of one ship by zone and plate.
    pub fn ship_armor(&self, args: &ShipArgs) -> Result<ShipArmor, QueryError> {
        let ship = self.resolve_ship(&args.ship)?;
        let plates = plates(ship);

        let mut zones: BTreeMap<&'static str, (Millimeters, Millimeters)> = BTreeMap::new();
        for plate in plates.iter().filter(|plate| is_above_water(plate.material)) {
            for thickness in plate.thickness_mm.iter().copied().filter(|mm| mm.value() > 0.0) {
                let entry = zones.entry(plate.zone).or_insert((thickness, thickness));
                if thickness.value() < entry.0.value() {
                    entry.0 = thickness;
                }
                if thickness.value() > entry.1.value() {
                    entry.1 = thickness;
                }
            }
        }
        let zones = zones.into_iter().map(|(zone, (min_mm, max_mm))| ZoneArmor { zone, min_mm, max_mm }).collect();

        Ok(ShipArmor { ship: ship.summary(), zones, plates })
    }
}

/// Plates of `ship` that the armor view classifies, sorted by zone and material.
pub(crate) fn plates(ship: &Ship) -> Vec<PlateArmor> {
    let Some(armor) = ship.param.vehicle().and_then(|vehicle| vehicle.armor()) else {
        return Vec::new();
    };

    let mut plates: Vec<PlateArmor> = Vec::new();
    for (material, layers) in armor {
        let Ok(material) = u8::try_from(*material) else {
            continue;
        };
        let name = collision_material_name(material);
        if !armor_type_classifies(name) {
            continue;
        }

        let mut thickness_mm: Vec<Millimeters> = layers.values().map(|mm| Millimeters::new(*mm)).collect();
        thickness_mm.sort_by(|left, right| left.value().total_cmp(&right.value()));
        thickness_mm.dedup();
        plates.push(PlateArmor { zone: zone_from_material_name(name), material: name, thickness_mm });
    }
    plates.sort_by(|left, right| left.zone.cmp(right.zone).then_with(|| left.material.cmp(right.material)));

    plates
}

/// Thickness to test a shell against. A plate name selects that plate's
/// thickest layer. A zone name such as "Bow" selects the thinnest nonzero
/// plate of that zone above the waterline, which is the plate a shell must
/// defeat to damage the zone.
pub(crate) fn zone_thickness(ship: &Ship, zone: &str) -> Result<ZoneThickness, QueryError> {
    let plates = plates(ship);

    let named = plates.iter().find(|plate| plate.material.eq_ignore_ascii_case(zone.trim()));
    if let Some(plate) = named
        && let Some(thickest) = plate.thickness_mm.iter().rev().find(|mm| mm.value() > 0.0)
    {
        return Ok(ZoneThickness { thickness: *thickest, plates: vec![plate.material] });
    }

    let zone_name = canonical_zone(zone);
    let mut thinnest: Option<ZoneThickness> = None;
    for plate in
        plates.iter().filter(|plate| plate.zone.eq_ignore_ascii_case(zone_name) && is_above_water(plate.material))
    {
        let Some(thickness) = plate.thickness_mm.iter().copied().find(|mm| mm.value() > 0.0) else {
            continue;
        };

        match &mut thinnest {
            Some(current) if current.thickness == thickness => current.plates.push(plate.material),
            Some(current) if current.thickness.value() < thickness.value() => {}
            _ => thinnest = Some(ZoneThickness { thickness, plates: vec![plate.material] }),
        }
    }

    thinnest.ok_or_else(|| {
        let mut zones: Vec<&'static str> = plates.iter().map(|plate| plate.zone).collect();
        zones.dedup();
        QueryError::ZoneNotFound { ship: ship.name.clone(), zone: zone.to_owned(), zones }
    })
}

fn is_above_water(material: &str) -> bool {
    !material.ends_with("_Bottom")
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
