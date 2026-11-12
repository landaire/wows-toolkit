//! Single-model loads over a real install. Ignored: needs an install named by
//! `WOWS_DIR`; returns early when that directory is absent.

use std::path::PathBuf;
use std::sync::LazyLock;

use wowsunpack::export::gltf_export::InteractiveHullMesh;
use wowsunpack::export::hardpoint::HardpointKind;
use wowsunpack::export::model_catalog::ModelPath;
use wowsunpack::export::model_library::LoadedModel;
use wowsunpack::export::model_library::ModelLibrary;
use wowsunpack::export::model_library::ModelLoadOptions;
use wowsunpack::export::model_library::TextureDecode;
use wowsunpack::export::model_library::TextureSource;
use wowsunpack::export::ship::ShipAssets;
use wowsunpack::export::texture::MaxEdge;
use wowsunpack::export::texture::TextureLod;

const IOWA: &str = "content/gameplay/usa/ship/battleship/ASB028_Iowa_1945/ASB028_Iowa_1945.model";
const IOWA_TURRET: &str = "content/gameplay/usa/gun/main/AGM034_16in50_Mk7/AGM034_16in50_Mk7.model";
const YAMATO_TURRET: &str = "content/gameplay/japan/gun/main/JGM178_460mm_45_Type94/JGM178_460mm_45_Type94.model";

static LIBRARY: LazyLock<Option<ModelLibrary>> = LazyLock::new(|| {
    let dir =
        std::env::var_os("WOWS_DIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"E:\WoWs\World_of_Warships"));
    if !dir.join("bin").is_dir() {
        eprintln!("skipped: no install at {}", dir.display());
        return None;
    }
    let assets = ShipAssets::from_game_dir(&dir).expect("ship assets");
    Some(assets.model_library().expect("model library"))
});

fn path(raw: &str) -> ModelPath {
    ModelPath::parse(raw).expect("valid model path")
}

fn options(textures: TextureDecode) -> ModelLoadOptions {
    ModelLoadOptions {
        textures,
        texture_lod: TextureLod::Capped(MaxEdge::new(256).expect("nonzero")),
        ..ModelLoadOptions::default()
    }
}

/// Mesh positions in node space: each mesh's own transform applied.
fn placed_positions(mesh: &InteractiveHullMesh) -> impl Iterator<Item = [f32; 3]> + '_ {
    mesh.positions.iter().map(|&[x, y, z]| match mesh.transform {
        Some(m) => [
            m[0] * x + m[4] * y + m[8] * z + m[12],
            m[1] * x + m[5] * y + m[9] * z + m[13],
            m[2] * x + m[6] * y + m[10] * z + m[14],
        ],
        None => [x, y, z],
    })
}

fn mesh_z_range(model: &LoadedModel) -> (f32, f32) {
    model
        .meshes()
        .iter()
        .flat_map(placed_positions)
        .fold((f32::MAX, f32::MIN), |(lo, hi), p| (lo.min(p[2]), hi.max(p[2])))
}

fn hardpoint_position(model: &LoadedModel, name: &str) -> [f32; 3] {
    model
        .hardpoints()
        .iter()
        .find(|h| h.name.as_str() == name)
        .unwrap_or_else(|| panic!("{name} is a hardpoint of {}", model.path()))
        .position()
}

/// The muzzle hardpoint must sit at the barrel end of the meshes. A mesh left
/// in its skinned bone frame would point the barrels the other way.
fn assert_barrels_reach_the_muzzle(model: &LoadedModel) {
    let muzzle = hardpoint_position(model, "HP_gunFire1");
    let (min_z, max_z) = mesh_z_range(model);
    let barrel_end = if muzzle[2] < 0.0 { min_z } else { max_z };
    assert!(
        (barrel_end - muzzle[2]).abs() < 0.35,
        "{}: muzzle z {} is not at the barrel end of meshes spanning {min_z}..{max_z}",
        model.path(),
        muzzle[2]
    );
}

#[test]
#[ignore = "requires a World of Warships install"]
fn a_turret_loads_with_textures_hardpoints_and_armor() {
    let Some(library) = LIBRARY.as_ref() else {
        return;
    };
    let turret = library.load_model(&path(IOWA_TURRET), &options(TextureDecode::AllMaps)).expect("turret loads");

    assert_eq!(turret.parts().len(), 1);
    assert!(!turret.meshes().is_empty());
    assert!(turret.lod_count() >= 4, "turrets author several LODs");
    assert!(turret.has_armor(), "main turrets carry armor");

    let material = turret
        .material("content/gameplay/usa/gun/main/textures/AGM034_16in50_Mk7_skinned.mfm")
        .expect("the turret material");
    let albedo = material.albedo.as_ref().expect("albedo");
    assert_eq!(
        albedo.source,
        TextureSource::MaterialProperty {
            path: "content/gameplay/usa/gun/main/textures/AGM034_16in50_Mk7_a.dds".to_string()
        }
    );
    let image = albedo.image.as_ref().expect("decoded albedo");
    assert!(image.width <= 256 && image.height <= 256, "decoded at the requested texture LOD");
    assert!(image.pixels.chunks_exact(4).all(|p| p[3] == u8::MAX), "albedo is opaque");
    let normal = material.normal.as_ref().expect("normal map");
    assert_eq!(normal.source.path(), Some("content/gameplay/usa/gun/main/textures/AGM034_16in50_Mk7_n.dds"));
    assert!(normal.image.is_some());
    let metal_gloss = material.metal_gloss.as_ref().expect("metal-gloss map");
    assert_eq!(metal_gloss.source.path(), Some("content/gameplay/usa/gun/main/textures/AGM034_16in50_Mk7_mg.dds"));

    // HP_AGA_1 hangs off Rotate_Y (y 0.0464) at (0.1499, 0.178, -0.501).
    let aa = hardpoint_position(&turret, "HP_AGA_1");
    let expected = [0.1499, 0.0464 + 0.178, 0.501];
    assert!(aa.iter().zip(expected).all(|(a, b)| (a - b).abs() < 1e-4), "HP_AGA_1 at {aa:?}, expected {expected:?}");
    assert_barrels_reach_the_muzzle(&turret);
}

#[test]
#[ignore = "requires a World of Warships install"]
fn a_reflected_turret_lines_meshes_up_with_its_nodes() {
    let Some(library) = LIBRARY.as_ref() else {
        return;
    };
    let turret = library.load_model(&path(YAMATO_TURRET), &options(TextureDecode::None)).expect("turret loads");

    assert!(turret.meshes().iter().all(|m| m.transform.is_some()), "Yamato's turret meshes are skinned reflected");
    assert!(turret.materials().values().all(|m| m.albedo.as_ref().is_some_and(|a| a.image.is_none())));
    assert_barrels_reach_the_muzzle(&turret);
}

#[test]
#[ignore = "requires a World of Warships install"]
fn a_ship_loads_its_hull_sections() {
    let Some(library) = LIBRARY.as_ref() else {
        return;
    };
    let started = std::time::Instant::now();
    let iowa = library.load_model(&path(IOWA), &options(TextureDecode::Albedo)).expect("Iowa loads");
    eprintln!("[measured] Iowa hull load: {:?}", started.elapsed());

    let parts: Vec<&str> = iowa.parts().iter().map(|p| p.model_path.stem()).collect();
    assert_eq!(
        parts,
        [
            "ASB028_Iowa_1945",
            "ASB028_Iowa_1945_Bow",
            "ASB028_Iowa_1945_MidBack",
            "ASB028_Iowa_1945_MidFront",
            "ASB028_Iowa_1945_Stern"
        ]
    );
    assert!(iowa.meshes().len() > 10);
    assert!(iowa.has_armor(), "the hull carries armor");
    for name in ["HP_AGM_1", "HP_AGM_2", "HP_AGM_3"] {
        hardpoint_position(&iowa, name);
    }
    assert!(iowa.hardpoints().iter().any(|h| h.kind == HardpointKind::Misc), "section extenders add misc nodes");
    assert!(iowa.hardpoints().iter().any(|h| h.kind == HardpointKind::Effect), "section extenders add effect nodes");

    // HP_AGM_1 is the forward turret, so it sits ahead of HP_AGM_3 along the
    // negated Z axis the meshes use.
    assert!(hardpoint_position(&iowa, "HP_AGM_1")[2] < hardpoint_position(&iowa, "HP_AGM_3")[2]);

    let sources: Vec<&TextureSource> =
        iowa.materials().values().filter_map(|m| m.albedo.as_ref()).map(|a| &a.source).collect();
    assert!(!sources.is_empty());
    eprintln!(
        "Iowa albedo sources: {} property, {} name match, {} baked, {} materials",
        sources.iter().filter(|s| matches!(s, TextureSource::MaterialProperty { .. })).count(),
        sources.iter().filter(|s| matches!(s, TextureSource::NameMatch { .. })).count(),
        sources.iter().filter(|s| matches!(s, TextureSource::TiledLandBake)).count(),
        iowa.materials().len()
    );
    let hull = iowa
        .material("content/gameplay/usa/ship/battleship/textures/ASB028_Iowa_1945.mfm")
        .and_then(|m| m.albedo.as_ref())
        .expect("hull albedo");
    assert_eq!(hull.source.path(), Some("content/gameplay/usa/ship/battleship/textures/ASB028_Iowa_1945_a.dds"));
}

#[test]
#[ignore = "requires a World of Warships install"]
fn every_catalogued_kind_loads() {
    let Some(library) = LIBRARY.as_ref() else {
        return;
    };
    let catalog = library.catalog();
    let mut failures = Vec::new();
    let mut loaded = 0usize;
    for kind in wowsunpack::export::model_catalog::ModelKind::ALL {
        for entry in catalog.entries().iter().filter(|e| e.kind() == kind).take(5) {
            match library.load_model(entry.path(), &options(TextureDecode::None)) {
                Ok(_) => loaded += 1,
                Err(error) => failures.push(format!("{}: {error}", entry.path())),
            }
        }
    }
    eprintln!("{loaded} loaded, {} failed", failures.len());
    assert!(failures.is_empty(), "{failures:#?}");
}
