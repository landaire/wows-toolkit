use std::collections::HashMap;

use wowsunpack::export::hardpoint::HardpointName;
use wowsunpack::export::model_catalog::BuildingKind;
use wowsunpack::export::model_catalog::BuildingModels;
use wowsunpack::export::model_catalog::BuildingParamRef;
use wowsunpack::export::model_catalog::CatalogLinks;
use wowsunpack::export::model_catalog::GeometryAvailability;
use wowsunpack::export::model_catalog::ModelCatalog;
use wowsunpack::export::model_catalog::ModelPath;
use wowsunpack::export::part_group::PartGroup;
use wowsunpack::export::ship_parts::ListedMisc;
use wowsunpack::export::ship_parts::ListedMount;
use wowsunpack::export::ship_parts::MiscParent;
use wowsunpack::export::ship_parts::ShipPartListing;
use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::game_params::types::PeculiarityModel;
use wowsunpack::game_params::types::Species;
use wowsunpack::game_types::GameParamId;

use crate::catalog::ClassGroup;
use crate::catalog::HiddenReason;
use crate::catalog::HiddenShip;
use crate::catalog::NationGroup;
use crate::catalog::ShipCatalog;
use crate::catalog::ShipEntry;
use crate::filter_fields::FilterFields;
use crate::model_refs::VehicleParamIndex;
use crate::model_tree::BuildingRow;
use crate::model_tree::FortificationRow;
use crate::model_tree::ModelBuildings;
use crate::model_tree::ModelReferences;
use crate::model_tree::PartKeys;
use crate::model_tree::ShipPartRows;

pub const IOWA_INDEX: &str = "PASB018";
pub const IOWA: &str = "content/gameplay/usa/ship/battleship/ASB028_Iowa_1945/ASB028_Iowa_1945.model";
pub const IOWA_TURRET: &str = "content/gameplay/usa/gun/main/AGM034_16in50_Mk7/AGM034_16in50_Mk7.model";
pub const IOWA_SKIN: &str = "content/gameplay/usa/ship/battleship/ASB128_Iowa_Halloween/ASB128_Iowa_Halloween.model";
pub const SECONDARY: &str = "content/gameplay/usa/gun/secondary/AGS010_5in38_Mk32_Mod12/AGS010_5in38_Mk32_Mod12.model";
const DIRECTOR: &str = "content/gameplay/usa/director/AD001_Director_Mk37/AD001_Director_Mk37.model";
const BOLLARD: &str = "content/gameplay/usa/misc/AM2061/AM2061.model";
const YAMATO: &str = "content/gameplay/japan/ship/battleship/JSB018_Yamato_1944/JSB018_Yamato_1944.model";
pub const YAMATO_TURRET: &str = "content/gameplay/japan/gun/main/JGM178_460mm_45_Type94/JGM178_460mm_45_Type94.model";
pub const FORT: &str = "content/location/gameplay/building/LGB001_Fort/LGB001_Fort.model";

pub fn path(raw: &str) -> ModelPath {
    ModelPath::parse(raw).expect("a model path")
}

/// A ship as `ShipCatalog::build` lists it, with real GameParams data.
fn ship(param_index: &str, short_name: &str, full_name: &str, param_name: &str, id: u64, tier: u32) -> ShipEntry {
    ShipEntry {
        param_index: param_index.to_string(),
        display_name: short_name.to_string(),
        filter_fields: key(&[short_name, full_name, param_index, param_name, &id.to_string()]),
        tier,
    }
}

fn key(fields: &[&str]) -> FilterFields {
    fields.iter().map(|field| field.to_string()).collect()
}

pub fn ships() -> ShipCatalog {
    let nation = |nation: &str, display_name: &str, ships: Vec<ShipEntry>| NationGroup {
        nation: nation.to_string(),
        display_name: display_name.to_string(),
        classes: vec![ClassGroup { species: Species::Battleship, ships }],
    };
    let yamato = ship("PJSB018", "Yamato", "Yamato", "PJSB018_Yamato_1944", 4276041424, 10);
    let south_carolina = ship("PASB001", "S. Carolina", "South Carolina", "PASB001_Michigan_1916", 4293867504, 3);
    let iowa = ship(IOWA_INDEX, "Iowa", "Iowa", "PASB018_Iowa_1944", 4276041712, 9);
    // A clan rental copy of Iowa, which the list leaves out.
    let rental = HiddenShip {
        nation: "USA".to_string(),
        nation_display_name: "U.S.A.".to_string(),
        species: Species::Battleship,
        ship: ship("PASB708", "[Iowa]", "[Iowa]", "PASB708_Iowa_1944_Clan", 4276041000, 9),
        reason: HiddenReason::ClanRental,
    };

    ShipCatalog {
        nations: vec![nation("Japan", "Japan", vec![yamato]), nation("USA", "U.S.A.", vec![south_carolina, iowa])],
        hidden: vec![rental],
    }
}

/// The ships' hull models and Iowa's mounts, with the game's names for
/// the guns.
pub fn references(ships: &ShipCatalog) -> ModelReferences {
    let ship_key = |index: &str| {
        ships.ships().find(|ship| ship.param_index == index).map(|ship| ship.filter_fields.clone()).expect("listed")
    };
    let mut references = ModelReferences::new();
    references.insert(IOWA, &ship_key(IOWA_INDEX));
    references.insert(YAMATO, &ship_key("PJSB018"));
    references
        .insert(IOWA_TURRET, &key(&["406 mm/50 Mk.7 in a turret", "PAGM034", "PAGM034_16in50_Mk7", "4258916336"]));
    references.insert(
        SECONDARY,
        &key(&["127 mm/38 Mk.12 on a Mk.32 mount", "PAGS010", "PAGS010_5in38_Mk32_Mod12", "4283885552"]),
    );
    references.insert(DIRECTOR, &key(&["PAD001", "PAD001_Director_Mk37", "4293063664"]));
    // Two exteriors, out of order, show the same skin.
    references.insert_skin(IOWA_SKIN, IOWA, "Spooky");
    references.insert_skin(IOWA_SKIN, IOWA, "Halloween");

    references
}

pub fn catalog() -> ModelCatalog {
    let paths = [
        IOWA,
        "content/gameplay/usa/ship/battleship/ASB028_Iowa_1945/ASB028_Iowa_1945_Bow.model",
        "content/gameplay/usa/ship/battleship/ASB028_Iowa_1945/ASB028_Iowa_1945_MidBack.model",
        "content/gameplay/usa/ship/battleship/ASB028_Iowa_1945/ASB028_Iowa_1945_MidFront.model",
        "content/gameplay/usa/ship/battleship/ASB028_Iowa_1945/ASB028_Iowa_1945_Stern.model",
        IOWA_SKIN,
        "content/gameplay/usa/ship/battleship/ASB128_Iowa_Halloween/ASB128_Iowa_Halloween_dead.model",
        IOWA_TURRET,
        "content/gameplay/usa/gun/main/AGM034_16in50_Mk7/AGM034_16in50_Mk7_dead.model",
        SECONDARY,
        DIRECTOR,
        BOLLARD,
        YAMATO,
        YAMATO_TURRET,
        "content/gameplay/events/director/XD006_Lazarus_Director/XD006_Lazarus_Director.model",
        "content/gameplay/usa/misc/AM116_Boat_Winch_2/AM116_Boat_Winch_2.model",
        FORT,
        "content/location/building/city/LBC001_House/LBC001_House.model",
        "content/ports/misc/wharf/PMW001_Crane/PMW001_Crane.model",
    ];
    let skins = vec![PeculiarityModel { stock: IOWA.to_string(), replacement: IOWA_SKIN.to_string() }];
    let fort = BuildingModels {
        param: BuildingParamRef::new("PCBX022_PCGM002".to_string(), BuildingKind::Complex),
        model: Some(FORT.to_string()),
        dead_model: None,
        mount_models: Vec::new(),
    };

    // The Japanese turret's geometry is not installed, nor the secondary
    // gun's that Iowa mounts; nothing else is known.
    let geometry = HashMap::from([
        (path(YAMATO_TURRET), GeometryAvailability::Absent),
        (path(SECONDARY), GeometryAvailability::NoGeometry),
    ]);

    ModelCatalog::from_paths(paths.map(path), &CatalogLinks::new(skins, vec![fort])).with_geometry(geometry)
}

pub const FORT_OF_THE_ISLES: u64 = 4229999600;
pub const COASTAL_BATTERY: u64 = 4229999601;

/// A fort the game names, armored in its hull.
pub fn fortifications() -> Vec<FortificationRow> {
    let (display_name, param_name) = ("Fort of the Isles", "PCBX001");

    vec![FortificationRow {
        id: GameParamId::from(FORT_OF_THE_ISLES),
        kind: BuildingKind::Complex,
        kind_name: Some("Fort".to_string()),
        display_name: display_name.to_string(),
        param_name: param_name.to_string(),
        filter_fields: key(&[display_name, param_name, &FORT_OF_THE_ISLES.to_string()]),
        // Its hull is not installed.
        geometry: Some(GeometryAvailability::Absent),
    }]
}

/// The fort and a coastal battery the game does not name, both on the
/// fort model.
pub fn model_buildings() -> ModelBuildings {
    let row = |id: u64, display_name: &str, param_name: &str| BuildingRow {
        id: GameParamId::from(id),
        display_name: display_name.to_string(),
        param_name: param_name.to_string(),
    };

    ModelBuildings::new(HashMap::from([(
        path(FORT),
        vec![row(FORT_OF_THE_ISLES, "Fort of the Isles", "PCBX001"), row(COASTAL_BATTERY, "PCBC002", "PCBC002")],
    )]))
}

/// A provider without params or translations: names the game would give
/// fall back to their keys.
pub fn metadata() -> GameMetadataProvider {
    GameMetadataProvider::from_params_no_specs(Vec::new()).expect("an empty provider builds")
}

/// Iowa's parts; Yamato lists none.
pub fn ship_parts(catalog: &ModelCatalog, references: &ModelReferences) -> HashMap<VehicleParamIndex, ShipPartRows> {
    let mount = |hp: &str, model: &str, group: PartGroup| ListedMount {
        hp: HardpointName::new(hp),
        model: path(model),
        species: None,
        group,
    };
    let listing = ShipPartListing {
        hull_sections: catalog
            .entry(&path(IOWA))
            .map(|entry| std::iter::once(entry.path().clone()).chain(entry.sections().cloned()).collect())
            .expect("Iowa is catalogued"),
        mounts: vec![
            mount("HP_AGM_1", IOWA_TURRET, PartGroup::MainBattery),
            mount("HP_AGM_2", IOWA_TURRET, PartGroup::MainBattery),
            mount("HP_AGS_1", SECONDARY, PartGroup::SecondaryBattery),
            mount("HP_AD_1", DIRECTOR, PartGroup::FireControl),
        ],
        miscs: vec![ListedMisc {
            node: HardpointName::new("MP_AM2061_1"),
            parent: MiscParent::HullSection(path(
                "content/gameplay/usa/ship/battleship/ASB028_Iowa_1945/ASB028_Iowa_1945_Bow.model",
            )),
            model: path(BOLLARD),
        }],
    };

    let rows = ShipPartRows::from_listing(&listing, catalog, &mut PartKeys::new(references));

    HashMap::from([(VehicleParamIndex::new(IOWA_INDEX), rows)])
}
