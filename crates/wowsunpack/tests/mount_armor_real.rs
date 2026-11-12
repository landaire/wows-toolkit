//! The mount armor reference index over a real install. Ignored: needs an
//! install named by `WOWS_DIR`; returns early when that directory is absent.

use std::path::PathBuf;

use wowsunpack::export::hardpoint::HardpointName;
use wowsunpack::export::model_catalog::ModelPath;
use wowsunpack::export::model_library::ModelLoadOptions;
use wowsunpack::export::model_library::TextureDecode;
use wowsunpack::export::mount_armor::MountArmorIndex;
use wowsunpack::export::mount_armor::VehicleParamIndex;
use wowsunpack::export::ship::ShipAssets;
use wowsunpack::export::ship::ShipExportOptions;
use wowsunpack::game_params::types::GameParamProvider;

const IOWA_DIR: &str = "/ASB028_Iowa_1945/";
const IOWA_TURRET: &str = "content/gameplay/usa/gun/main/AGM034_16in50_Mk7/AGM034_16in50_Mk7.model";
const STOCK_TURRET: &str = "content/gameplay/germany/gun/main/GGM058_280mm54_5_C34/GGM058_280mm54_5_C34.model";
const SKIN_TURRET: &str =
    "content/gameplay/germany/gun/main/GGM546_280mm54_5_C34_Black/GGM546_280mm54_5_C34_Black.model";

/// Sorted plate thicknesses in tenths of a millimetre, for comparing meshes.
fn thicknesses(meshes: &[wowsunpack::export::gltf_export::InteractiveArmorMesh]) -> Vec<i64> {
    let mut values: Vec<i64> = meshes
        .iter()
        .flat_map(|mesh| &mesh.triangle_info)
        .map(|info| (f64::from(info.thickness_mm) * 10.0).round() as i64)
        .collect();
    values.sort_unstable();

    values
}

#[test]
#[ignore = "requires a World of Warships install"]
fn a_turret_opened_alone_shows_the_thickness_its_ship_gives_it() {
    let dir =
        std::env::var_os("WOWS_DIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"E:\WoWs\World_of_Warships"));
    if !dir.join("bin").is_dir() {
        eprintln!("skipped: no install at {}", dir.display());
        return;
    }
    let assets = ShipAssets::from_game_dir(&dir).expect("ship assets");

    let library = assets.model_library().expect("model library");
    let started = std::time::Instant::now();
    let index = MountArmorIndex::build(assets.metadata(), library.catalog());
    eprintln!("[measured] index of {} mounted models built in {:?}", index.len(), started.elapsed());

    let turret = ModelPath::parse(IOWA_TURRET).expect("model path");
    let refs = index.referencing(&turret);
    assert!(!refs.is_empty(), "Iowa's turret is mounted");
    let tiers: Vec<u32> = refs
        .iter()
        .map(|mount| {
            let param =
                assets.metadata().game_param_by_index(mount.vehicle_index().as_str()).expect("indexed vehicle exists");
            param.vehicle().expect("a vehicle").level()
        })
        .collect();
    assert!(tiers.is_sorted(), "ordered by tier, got {tiers:?}");

    let iowa_param = assets
        .metadata()
        .params()
        .iter()
        .find(|param| param.vehicle().and_then(|v| v.model_path()).is_some_and(|path| path.contains(IOWA_DIR)))
        .expect("Iowa in GameParams")
        .clone();
    let hp = HardpointName::new("HP_AGM_1");
    let mount =
        index.find(&turret, &VehicleParamIndex::new(iowa_param.index()), &hp).expect("Iowa mounts it at HP_AGM_1");
    assert_eq!(mount.vehicle(), iowa_param.id());
    assert!(mount.mount_armor().is_some(), "a main turret has its own armor map");
    assert!(mount.hull_armor().is_some());

    let options = ModelLoadOptions { textures: TextureDecode::None, ..ModelLoadOptions::default() };
    let alone = library.load_model(&turret, &options).expect("turret loads");
    let alone_armor = alone.armor_meshes(mount.hull_armor(), mount.mount_armor());
    let alone_thickness = thicknesses(&alone_armor);
    assert!(alone_thickness.iter().any(|&t| t > 0), "plates have thickness");

    let vehicle = iowa_param.vehicle().expect("a vehicle");
    let options = ShipExportOptions { textures: false, ..ShipExportOptions::default() };
    let ship = assets.load_ship_from_vehicle(vehicle, &options).expect("Iowa loads");
    let mounted: Vec<_> = ship
        .interactive_armor_meshes()
        .expect("ship armor")
        .into_iter()
        .filter(|mesh| mesh.name.ends_with("[HP_AGM_1]"))
        .collect();
    assert_eq!(alone_thickness, thicknesses(&mounted), "the same plates read the same on the ship");
}

#[test]
#[ignore = "requires a World of Warships install"]
fn a_skin_borrows_the_mounts_of_its_stock_model() {
    let dir =
        std::env::var_os("WOWS_DIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"E:\WoWs\World_of_Warships"));
    if !dir.join("bin").is_dir() {
        eprintln!("skipped: no install at {}", dir.display());
        return;
    }
    let assets = ShipAssets::from_game_dir(&dir).expect("ship assets");
    let library = assets.model_library().expect("model library");
    let index = MountArmorIndex::build(assets.metadata(), library.catalog());

    let stock = ModelPath::parse(STOCK_TURRET).expect("model path");
    let skin = ModelPath::parse(SKIN_TURRET).expect("model path");
    assert_eq!(library.catalog().entry(&skin).expect("skin catalogued").skin_of(), std::slice::from_ref(&stock));
    let stock_hps: Vec<(&VehicleParamIndex, &HardpointName)> =
        index.referencing(&stock).iter().map(|m| (m.vehicle_index(), m.hp())).collect();
    let skin_hps: Vec<(&VehicleParamIndex, &HardpointName)> =
        index.referencing(&skin).iter().map(|m| (m.vehicle_index(), m.hp())).collect();
    assert!(!stock_hps.is_empty(), "the stock turret is mounted");
    assert_eq!(skin_hps, stock_hps);
}
