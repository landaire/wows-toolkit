//! Armored buildings over a real install. Ignored: needs an install named by
//! `WOWS_DIR`; returns early when that directory is absent.

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::io::Read;
use std::path::PathBuf;

use wowsunpack::export::building_armor::ArmoredBuilding;
use wowsunpack::export::building_armor::BuildingArmorIndex;
use wowsunpack::export::model_catalog::BuildingKind;
use wowsunpack::export::model_catalog::GeometryAvailability;
use wowsunpack::export::model_library::ModelExportOptions;
use wowsunpack::export::model_library::ModelLoadOptions;
use wowsunpack::export::model_library::TextureDecode;
use wowsunpack::export::ship::ExportContents;
use wowsunpack::export::ship::ShipAssets;
use wowsunpack::export::texture::TextureLod;
use wowsunpack::models::geometry;

fn assets() -> Option<ShipAssets> {
    let dir =
        std::env::var_os("WOWS_DIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"E:\WoWs\World_of_Warships"));
    if !dir.join("bin").is_dir() {
        eprintln!("skipped: no install at {}", dir.display());
        return None;
    }

    Some(ShipAssets::from_game_dir(&dir).expect("ship assets"))
}

fn building<'a>(index: &'a BuildingArmorIndex, name: &str) -> &'a ArmoredBuilding {
    index
        .buildings()
        .iter()
        .find(|building| building.param().name() == name)
        .unwrap_or_else(|| panic!("{name} is an armored building"))
}

/// Forts PCBX006 and PCBX010 share hull model LYB016. Its `hit_locations`
/// collision model has 157 polygons, each keyed into the fort's `hull.armor`.
#[test]
#[ignore = "requires a World of Warships install"]
fn fort_hull_plates_take_their_thickness_from_the_fort() {
    let Some(assets) = assets() else {
        return;
    };
    let library = assets.model_library().expect("model library");
    let index = BuildingArmorIndex::build(assets.metadata(), &library);

    for (name, belt_mm) in [("PCBX006", 200.0), ("PCBX010", 100.0)] {
        let fort = building(&index, name);
        assert!(fort.hull_has_armor(), "{name}'s hull is armored");
        let model = fort.model().expect("a hull model");
        assert_eq!(model.stem(), "LYB016");

        let options =
            ModelLoadOptions { lod: 0, damaged: false, texture_lod: TextureLod::Full, textures: TextureDecode::None };
        let loaded = library.load_model(model, &options).expect("LYB016 loads");

        let mut geometry_bytes = Vec::new();
        library
            .vfs()
            .join(&loaded.parts()[0].geometry_path)
            .and_then(|path| path.open_file())
            .expect("LYB016 geometry")
            .read_to_end(&mut geometry_bytes)
            .expect("LYB016 geometry reads");
        let geometry = geometry::parse_geometry(&geometry_bytes).expect("LYB016 geometry parses");
        let hit_locations = geometry
            .collision_models
            .iter()
            .find(|model| model.name == geometry::HIT_LOCATIONS_COLLISION_MODEL)
            .expect("a hit_locations collision model");
        let polygons: usize = hit_locations.hulls().expect("hulls decode").iter().map(|hull| hull.polygons.len()).sum();
        assert_eq!(polygons, 157);

        let maps = fort.hull_armor_maps();
        let meshes = loaded.armor_meshes(maps.hull, maps.mount);
        let hull =
            meshes.iter().find(|mesh| mesh.name == geometry::HIT_LOCATIONS_COLLISION_MODEL).expect("hull armor mesh");
        let armor = fort.armor().expect("hull.armor");
        let mut keys = BTreeSet::new();
        for info in &hull.triangle_info {
            let listed = armor.get(&u32::from(info.material_id)).and_then(|layers| layers.get(&info.model_index));
            assert_eq!(listed, Some(&info.thickness_mm), "{name} {:?} layer {}", info.material, info.model_index);
            keys.insert((info.material_id, info.model_index));
        }
        eprintln!("{name}: {} triangles over keys {keys:?}", hull.triangle_info.len());
        assert!(keys.contains(&(77, 1)), "OCit_Belt layer 1 is on the hull");
        let belt = hull.triangle_info.iter().find(|info| (info.material_id, info.model_index) == (77, 1)).unwrap();
        assert_eq!(belt.thickness_mm, belt_mm);

        // The plates sit in the hull's own space: inside the box of its
        // render meshes once those are placed on their nodes.
        let (mesh_min, mesh_max) = bounds(loaded.meshes().iter().flat_map(|mesh| {
            mesh.positions.iter().map(move |&position| mesh.transform.map_or(position, |m| transform(&m, position)))
        }));
        let (plate_min, plate_max) = bounds(hull.positions.iter().copied());
        eprintln!("{name}: meshes {mesh_min:?}..{mesh_max:?}, plates {plate_min:?}..{plate_max:?}");
        for axis in 0..3 {
            assert!(plate_min[axis] >= mesh_min[axis] - 0.1 && plate_max[axis] <= mesh_max[axis] + 0.1, "axis {axis}");
        }
    }
}

fn bounds(points: impl Iterator<Item = [f32; 3]>) -> ([f32; 3], [f32; 3]) {
    points.fold(([f32::MAX; 3], [f32::MIN; 3]), |(min, max), point| {
        (std::array::from_fn(|axis| min[axis].min(point[axis])), std::array::from_fn(|axis| max[axis].max(point[axis])))
    })
}

/// `matrix` (column-major) applied to `point`.
fn transform(matrix: &[f32; 16], point: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|row| {
        matrix[row] * point[0] + matrix[4 + row] * point[1] + matrix[8 + row] * point[2] + matrix[12 + row]
    })
}

/// Every hull plate of every shipped building model has a thickness in the
/// param's `hull.armor`: the map is keyed by the plates' collision keys.
#[test]
#[ignore = "requires a World of Warships install"]
fn every_building_hull_plate_is_in_its_armor_map() {
    let Some(assets) = assets() else {
        return;
    };
    let library = assets.model_library().expect("model library");
    let index = BuildingArmorIndex::build(assets.metadata(), &library);
    let options =
        ModelLoadOptions { lod: 0, damaged: false, texture_lod: TextureLod::Full, textures: TextureDecode::None };

    let mut checked = 0;
    for building in index.buildings().iter().filter(|building| building.hull_has_armor()) {
        let model = building.model().expect("an armored hull has a model");
        if library.catalog().geometry(model) != Some(GeometryAvailability::Present) {
            eprintln!("{} {}: not shipped", building.param().name(), model.stem());
            continue;
        }
        let loaded = library.load_model(model, &options).expect("a shipped building model loads");
        let armor = building.armor().expect("an armored hull has a map");
        let plates = loaded
            .armor_meshes(None, None)
            .into_iter()
            .filter(|mesh| mesh.name == geometry::HIT_LOCATIONS_COLLISION_MODEL)
            .flat_map(|mesh| mesh.triangle_info);
        let mut unlisted = BTreeSet::new();
        for info in plates {
            let listed =
                armor.get(&u32::from(info.material_id)).is_some_and(|layers| layers.contains_key(&info.model_index));
            if !listed {
                unlisted.insert((info.material_id, info.model_index));
            }
        }
        assert!(
            unlisted.is_empty(),
            "{} {} plates without thickness: {unlisted:?}",
            building.param().name(),
            model.stem()
        );
        checked += 1;
    }
    eprintln!("{checked} shipped armored hulls checked");
    assert!(checked > 40, "most building hulls ship");
}

#[test]
#[ignore = "requires a World of Warships install"]
fn a_fort_glb_holds_its_hull_armor() {
    let Some(assets) = assets() else {
        return;
    };
    let library = assets.model_library().expect("model library");
    let index = BuildingArmorIndex::build(assets.metadata(), &library);
    let fort = building(&index, "PCBX006");

    let options = ModelExportOptions {
        lod: 0,
        damaged: false,
        texture_lod: TextureLod::Full,
        textures: false,
        contents: ExportContents::Armor,
        hardpoints: false,
    };
    let mut glb = Vec::new();
    library
        .export_glb(fort.model().expect("a hull model"), &options, fort.hull_armor_maps(), &mut glb)
        .expect("export");

    let document = gltf::Gltf::from_slice(&glb).expect("parse glb");
    let armor_meshes: Vec<&str> =
        document.meshes().filter_map(|mesh| mesh.name()).filter(|name| name.starts_with("Armor_")).collect();
    assert!(!armor_meshes.is_empty(), "the hull plates are exported");
}

#[test]
#[ignore = "requires a World of Warships install"]
fn forts_list_their_armored_mounts_with_thickness() {
    let Some(assets) = assets() else {
        return;
    };
    let library = assets.model_library().expect("model library");

    let started = std::time::Instant::now();
    let index = BuildingArmorIndex::build(assets.metadata(), &library);
    eprintln!("[measured] armored buildings found in {:?}", started.elapsed());

    let mut kinds: BTreeMap<&BuildingKind, Vec<&str>> = BTreeMap::new();
    for building in index.buildings() {
        kinds.entry(building.kind()).or_default().push(building.param().name());
        let armored: Vec<String> = building
            .mounts()
            .iter()
            .filter(|mount| mount.has_armor())
            .map(|mount| format!("{} {} (map: {})", mount.hardpoint(), mount.model().stem(), mount.armor().is_some()))
            .collect();
        eprintln!(
            "  {} {:?}: hull {:?} plates {}, hull map {}, armored mounts {armored:?}",
            building.param().name(),
            building.kind(),
            building.model().map(|model| model.stem()),
            building.hull_has_armor(),
            building.armor().is_some()
        );
    }
    for (kind, names) in &kinds {
        eprintln!("{kind:?}: {}", names.len());
    }
    assert!(!index.buildings().is_empty(), "some buildings carry armor");

    let (fort, mount) = index
        .buildings()
        .iter()
        .filter(|building| building.kind() == &BuildingKind::Complex)
        .find_map(|building| Some((building, building.mounts().iter().find(|mount| mount.has_armor())?)))
        .expect("a fort with an armored mount");
    assert_eq!(index.building(fort.id()).map(|b| b.param()), Some(fort.param()));
    let thickest = mount
        .armor()
        .into_iter()
        .flat_map(|map| map.values())
        .flat_map(|layers| layers.values())
        .copied()
        .fold(0.0f32, f32::max);
    assert!(thickest > 0.0, "{} {} lists thickness", fort.param().name(), mount.hardpoint());
}
