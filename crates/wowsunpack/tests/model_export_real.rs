//! Single-model and ship GLB exports over a real install, read back with
//! `gltf`. Ignored: needs an install named by `WOWS_DIR`; returns early when
//! that directory is absent.

use std::path::PathBuf;

use wowsunpack::export::hardpoint::HardpointName;
use wowsunpack::export::model_catalog::ModelPath;
use wowsunpack::export::model_library::ModelArmorMaps;
use wowsunpack::export::model_library::ModelExportOptions;
use wowsunpack::export::model_library::ModelLoadOptions;
use wowsunpack::export::model_library::TextureDecode;
use wowsunpack::export::mount_armor::MountArmorIndex;
use wowsunpack::export::ship::ExportContents;
use wowsunpack::export::ship::ShipAssets;
use wowsunpack::export::ship::ShipExportOptions;
use wowsunpack::export::texture::MaxEdge;
use wowsunpack::export::texture::TextureLod;
use wowsunpack::game_params::types::GameParamProvider;

const IOWA_DIR: &str = "/ASB028_Iowa_1945/";
const IOWA_TURRET: &str = "content/gameplay/usa/gun/main/AGM034_16in50_Mk7/AGM034_16in50_Mk7.model";

fn assets() -> Option<ShipAssets> {
    let dir =
        std::env::var_os("WOWS_DIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"E:\WoWs\World_of_Warships"));
    if !dir.join("bin").is_dir() {
        eprintln!("skipped: no install at {}", dir.display());
        return None;
    }

    Some(ShipAssets::from_game_dir(&dir).expect("ship assets"))
}

fn top_node<'a>(document: &'a gltf::Document, name: &str) -> gltf::Node<'a> {
    let scene = document.default_scene().expect("a default scene");
    scene.nodes().find(|node| node.name() == Some(name)).unwrap_or_else(|| panic!("no top-level {name} node"))
}

#[test]
#[ignore = "requires a World of Warships install"]
fn a_turret_exports_with_textures_armor_and_hardpoint_nodes() {
    let Some(assets) = assets() else {
        return;
    };
    let library = assets.model_library().expect("model library");
    let index = MountArmorIndex::build(assets.metadata(), library.catalog());
    let turret = ModelPath::parse(IOWA_TURRET).expect("model path");
    let mount = index.referencing(&turret).first().expect("the turret is mounted");

    let options = ModelExportOptions {
        lod: 0,
        damaged: false,
        texture_lod: TextureLod::Capped(MaxEdge::new(256).expect("nonzero")),
        textures: true,
        contents: ExportContents::MeshAndArmor,
        hardpoints: true,
    };
    let armor = ModelArmorMaps { hull: mount.hull_armor(), mount: mount.mount_armor() };
    let mut glb = Vec::new();
    library.export_glb(&turret, &options, armor, &mut glb).expect("export");

    let gltf = gltf::Gltf::from_slice(&glb).expect("a valid GLB");
    let document = &gltf.document;

    assert!(document.images().count() > 0, "albedo is embedded");
    assert!(
        document.materials().any(|material| material.pbr_metallic_roughness().base_color_texture().is_some()),
        "a material samples the albedo"
    );

    let armor = top_node(document, "Armor");
    assert!(
        armor.children().any(|node| node.name().is_some_and(|n| n.starts_with("Armor_")) && node.mesh().is_some()),
        "armor zones are meshes"
    );

    let loaded = library
        .load_model(&turret, &ModelLoadOptions { textures: TextureDecode::None, ..ModelLoadOptions::default() })
        .expect("turret loads");
    let set = top_node(document, "Hardpoints").children().next().expect("the model's hardpoint set");
    assert_eq!(set.name(), Some(turret.stem()));
    let empties: Vec<gltf::Node<'_>> = set.children().collect();
    assert_eq!(empties.len(), loaded.hardpoints().len(), "one empty per hardpoint");
    for hardpoint in loaded.hardpoints() {
        let empty = empties
            .iter()
            .find(|node| node.name() == Some(hardpoint.name.as_str()))
            .unwrap_or_else(|| panic!("no node for {}", hardpoint.name));
        assert!(empty.mesh().is_none(), "{} is an empty", hardpoint.name);
        let matrix = empty.transform().matrix();
        assert_eq!(
            [matrix[3][0], matrix[3][1], matrix[3][2]],
            hardpoint.position(),
            "{} sits on its node",
            hardpoint.name
        );
    }

    let mesh_only =
        ModelExportOptions { textures: false, contents: ExportContents::Mesh, hardpoints: false, ..options };
    let mut plain = Vec::new();
    library.export_glb(&turret, &mesh_only, ModelArmorMaps::default(), &mut plain).expect("export");
    let plain = gltf::Gltf::from_slice(&plain).expect("a valid GLB");
    let scene = plain.document.default_scene().expect("a default scene");
    assert!(scene.nodes().all(|node| !matches!(node.name(), Some("Armor" | "Hardpoints"))));
    assert_eq!(plain.document.images().count(), 0);
}

#[test]
#[ignore = "requires a World of Warships install"]
fn a_ship_exports_its_hardpoints_per_part() {
    let Some(assets) = assets() else {
        return;
    };
    let vehicle = assets
        .metadata()
        .params()
        .iter()
        .filter_map(|param| param.vehicle())
        .find(|vehicle| vehicle.model_path().is_some_and(|path| path.contains(IOWA_DIR)))
        .expect("Iowa in GameParams")
        .clone();
    let options =
        ShipExportOptions { textures: false, contents: ExportContents::MeshAndArmor, ..ShipExportOptions::default() };
    let ship = assets.load_ship_from_vehicle(&vehicle, &options).expect("Iowa loads");
    let mut glb = Vec::new();
    ship.export_glb(&mut glb).expect("export");

    let gltf = gltf::Gltf::from_slice(&glb).expect("a valid GLB");
    let sets: Vec<gltf::Node<'_>> = top_node(&gltf.document, "Hardpoints").children().collect();
    let with_hardpoints = ship.parts().iter().filter(|part| !part.hardpoints.is_empty()).count();
    assert_eq!(sets.len(), with_hardpoints, "one set per part with hardpoints");

    let turret = ship
        .parts()
        .iter()
        .find(|part| part.key.label() == HardpointName::new("HP_AGM_1").as_str())
        .expect("forward turret");
    let set = sets.iter().find(|node| node.name() == Some("HP_AGM_1")).expect("the turret's set");
    let names: Vec<&str> = set.children().filter_map(|node| node.name()).collect();
    for hardpoint in &turret.hardpoints {
        assert!(names.contains(&hardpoint.name.as_str()), "{} is exported", hardpoint.name);
    }

    let without = ShipExportOptions { hardpoints: false, ..options };
    let ship = assets.load_ship_from_vehicle(&vehicle, &without).expect("Iowa loads");
    let mut glb = Vec::new();
    ship.export_glb(&mut glb).expect("export");
    let gltf = gltf::Gltf::from_slice(&glb).expect("a valid GLB");
    let scene = gltf.document.default_scene().expect("a default scene");
    assert!(scene.nodes().all(|node| node.name() != Some("Hardpoints")), "hardpoints are left out on request");
}
