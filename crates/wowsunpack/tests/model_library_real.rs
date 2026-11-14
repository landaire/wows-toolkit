//! Single-model loads over a real install. Ignored: needs an install named by
//! `WOWS_DIR`; returns early when that directory is absent.

use std::path::PathBuf;
use std::sync::LazyLock;

use rayon::iter::IntoParallelRefIterator;
use rayon::iter::ParallelIterator;

use wowsunpack::export::gltf_export::InteractiveHullMesh;
use wowsunpack::export::hardpoint::HardpointKind;
use wowsunpack::export::model_catalog::ModelPath;
use wowsunpack::export::model_library::LoadedModel;
use wowsunpack::export::model_library::ModelLibrary;
use wowsunpack::export::model_library::ModelLoadError;
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum SurveyOutcome {
    Loaded,
    GeometryAbsent,
    NoGeometry,
    SectionStandIn,
}

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
    assert!(image.pixels.as_chunks::<4>().0.iter().all(|p| p[3] == u8::MAX), "albedo is opaque");
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
        let of_kind = catalog.entries().iter().filter(|e| e.group().gameplay().is_some_and(|g| g.kind() == kind));
        for entry in of_kind.take(5) {
            match library.load_model(entry.path(), &options(TextureDecode::None)) {
                Ok(_) => loaded += 1,
                Err(error) => failures.push(format!("{}: {error}", entry.path())),
            }
        }
    }
    eprintln!("{loaded} loaded, {} failed", failures.len());
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
#[ignore = "requires a World of Warships install"]
fn a_lod_past_a_part_s_deepest_draws_that_part_at_its_deepest() {
    let Some(library) = LIBRARY.as_ref() else {
        return;
    };
    let iowa = library.load_model(&path(IOWA), &options(TextureDecode::None)).expect("Iowa loads");
    let deepest = ModelLoadOptions { lod: iowa.lod_count() - 1, ..options(TextureDecode::None) };
    let at_deepest = library.load_model(&path(IOWA), &deepest).expect("Iowa loads at its deepest LOD");
    let past = ModelLoadOptions { lod: 64, ..options(TextureDecode::None) };
    let clamped = library.load_model(&path(IOWA), &past).expect("Iowa loads past its LODs");

    assert!(!clamped.meshes().is_empty(), "each part clamps to its own deepest LOD");
    assert_eq!(clamped.meshes().len(), at_deepest.meshes().len());
}

#[test]
#[ignore = "requires a World of Warships install"]
fn a_summary_matches_the_loaded_model() {
    let Some(library) = LIBRARY.as_ref() else {
        return;
    };
    for raw in [IOWA, IOWA_TURRET] {
        let loaded = library.load_model(&path(raw), &options(TextureDecode::None)).expect("model loads");
        let summary = library.model_summary(&path(raw)).expect("summary reads");
        assert_eq!(summary.lod_count, loaded.lod_count(), "{raw}");
        assert_eq!(summary.has_armor, loaded.has_armor(), "{raw}");
    }
}

/// Models this install's catalogue lists without shipping their geometry:
/// one absent from every package, one indexed with no data (build 13533378).
const HARPOON: &str = "content/gameplay/usa/projectile/missile/API2001_RGM_84_Harpoon/API2001_RGM_84_Harpoon.model";
const HALLOWEEN_BOMB: &str = "content/gameplay/events/projectile/bomb/XPB007_Bomb_HW2018/XPB007_Bomb_HW2018.model";

#[test]
#[ignore = "requires a World of Warships install"]
fn a_model_without_shipped_geometry_reports_it_absent() {
    let Some(library) = LIBRARY.as_ref() else {
        return;
    };
    for raw in [HARPOON, HALLOWEEN_BOMB] {
        let Err(error) = library.load_model(&path(raw), &options(TextureDecode::None)) else {
            panic!("{raw} loads");
        };
        assert!(matches!(error.current_context(), ModelLoadError::GeometryAbsent { .. }), "{raw}: {error}");
    }
}

/// Loads every catalogued model and every hull section on its own. A load
/// may fail only for geometry the install does not ship, a visual that names
/// none, or a section that is a stand-in; every load has finite vertices.
#[test]
#[ignore = "requires a World of Warships install"]
fn every_catalogued_model_loads_unless_its_geometry_is_absent() {
    let Some(library) = LIBRARY.as_ref() else {
        return;
    };
    let threads = std::thread::available_parallelism().map_or(1, |count| (count.get() / 2).max(1));
    let pool = rayon::ThreadPoolBuilder::new().num_threads(threads).build().expect("thread pool");
    let entries = library.catalog().entries();
    let paths: Vec<&ModelPath> =
        entries.iter().flat_map(|entry| std::iter::once(entry.path()).chain(entry.sections())).collect();

    let outcomes: Vec<Result<SurveyOutcome, String>> = pool.install(|| {
        paths
            .par_iter()
            .map(|&path| {
                let model = match library.load_model(path, &options(TextureDecode::None)) {
                    Ok(model) => model,
                    Err(error) => {
                        return match error.current_context() {
                            ModelLoadError::GeometryAbsent { .. } => Ok(SurveyOutcome::GeometryAbsent),
                            ModelLoadError::NoGeometry { .. } => Ok(SurveyOutcome::NoGeometry),
                            ModelLoadError::SectionStandIn { .. } => Ok(SurveyOutcome::SectionStandIn),
                            _ => Err(format!("{path}: {error}")),
                        };
                    }
                };
                if model.meshes().iter().all(|mesh| mesh.indices.is_empty()) {
                    return Err(format!("{path}: no triangles"));
                }
                let finite = model.meshes().iter().flat_map(placed_positions).flatten().all(f32::is_finite);
                if !finite {
                    return Err(format!("{path}: a vertex is not finite"));
                }

                Ok(SurveyOutcome::Loaded)
            })
            .collect()
    });

    let failures: Vec<&String> = outcomes.iter().filter_map(|outcome| outcome.as_ref().err()).collect();
    let count =
        |wanted: SurveyOutcome| outcomes.iter().filter(|outcome| outcome.as_ref().ok() == Some(&wanted)).count();
    eprintln!(
        "{} models and sections: {} without shipped geometry, {} naming none, {} stand-in sections, {} failed",
        outcomes.len(),
        count(SurveyOutcome::GeometryAbsent),
        count(SurveyOutcome::NoGeometry),
        count(SurveyOutcome::SectionStandIn),
        failures.len()
    );
    assert!(failures.is_empty(), "{failures:#?}");
}

const BEARN: &str = "content/gameplay/france/ship/aircarrier/FSA001_Bearn_1936/FSA001_Bearn_1936.model";
const BEARN_BOW: &str = "content/gameplay/france/ship/aircarrier/FSA001_Bearn_1936/FSA001_Bearn_1936_Bow.model";
const BEARN_MID_BACK: &str =
    "content/gameplay/france/ship/aircarrier/FSA001_Bearn_1936/FSA001_Bearn_1936_MidBack.model";

/// Bearn authors its whole hull in the bow section and keeps a stand-in box
/// in each of the others.
#[test]
#[ignore = "requires a World of Warships install"]
fn a_stand_in_hull_section_is_reported_rather_than_drawn() {
    let Some(library) = LIBRARY.as_ref() else {
        return;
    };
    let Err(error) = library.load_model(&path(BEARN_MID_BACK), &options(TextureDecode::None)) else {
        panic!("the stand-in loads");
    };
    let bow = library.load_model(&path(BEARN_BOW), &options(TextureDecode::None)).expect("the bow loads");
    let ship = library.load_model(&path(BEARN), &options(TextureDecode::None)).expect("the ship loads");

    assert!(matches!(error.current_context(), ModelLoadError::SectionStandIn { .. }), "{error}");
    let (back, front) = mesh_z_range(&bow);
    assert!(front - back > 10.0, "the bow section spans the hull: {back}..{front}");
    assert!(ship.parts().iter().any(|part| part.model_path.as_str() == BEARN_MID_BACK), "the ship still loads it");
}

const CURTISS_TS1: &str =
    "content/gameplay/usa/aircraft/fighter/AAF014_Curtiss_TS1_float/AAF014_Curtiss_TS1_float.model";

/// The TS-1's propeller disc is stored at the origin of its `disc` node,
/// which sits under `propeller_mod` at the nose.
#[test]
#[ignore = "requires a World of Warships install"]
fn a_render_set_bound_to_a_node_lands_on_that_node() {
    let Some(library) = LIBRARY.as_ref() else {
        return;
    };
    let plane = library.load_model(&path(CURTISS_TS1), &options(TextureDecode::None)).expect("the TS-1 loads");
    let disc = plane.meshes().iter().find(|mesh| mesh.name == "discShape").expect("a propeller disc");

    let count = disc.positions.len() as f32;
    let center = disc.positions.iter().fold([0.0f32; 3], |sum, p| [sum[0] + p[0], sum[1] + p[1], sum[2] + p[2]]);
    let center = center.map(|c| c / count);
    let propeller = [0.0, 0.136, -0.083];
    let distance = (0..3).map(|axis| (center[axis] - propeller[axis]).powi(2)).sum::<f32>().sqrt();
    assert!(distance < 0.01, "the disc centres on the propeller node: {center:?}");
}
