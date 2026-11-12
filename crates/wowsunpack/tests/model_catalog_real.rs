//! The model catalogue over a real install. Ignored: needs an install named by
//! `WOWS_DIR`; returns early when that directory is absent.

use std::collections::BTreeMap;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::LazyLock;

use wowsunpack::export::model_catalog::BuildingKind;
use wowsunpack::export::model_catalog::BuildingParamRef;
use wowsunpack::export::model_catalog::CatalogLinks;
use wowsunpack::export::model_catalog::GeometryAvailability;
use wowsunpack::export::model_catalog::MiscCategory;
use wowsunpack::export::model_catalog::ModelCatalog;
use wowsunpack::export::model_catalog::ModelGroup;
use wowsunpack::export::model_catalog::ModelGroupKind;
use wowsunpack::export::model_catalog::ModelKind;
use wowsunpack::export::model_catalog::ModelPath;
use wowsunpack::export::model_catalog::ModelPathIndex;
use wowsunpack::export::model_catalog::ModelSubKind;
use wowsunpack::export::model_catalog::ModelVariantKind;
use wowsunpack::export::model_library::ModelLoadError;
use wowsunpack::export::model_library::ModelLoadOptions;
use wowsunpack::export::model_library::TextureDecode;
use wowsunpack::export::ship::ShipAssets;
use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::models::assets_bin;

struct Install {
    index: ModelPathIndex,
    links: CatalogLinks,
    catalog: ModelCatalog,
}

static INSTALL: LazyLock<Option<Install>> = LazyLock::new(|| {
    let dir =
        std::env::var_os("WOWS_DIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"E:\WoWs\World_of_Warships"));
    if !dir.join("bin").is_dir() {
        eprintln!("skipped: no install at {}", dir.display());
        return None;
    }

    let vfs = wowsunpack::game_data::build_game_vfs(&dir, wowsunpack::game_data::AssetsBin::Omit).expect("game vfs");
    let mut bytes = Vec::new();
    let mut file = vfs.join("content/assets.bin").expect("assets.bin path").open_file().expect("open assets.bin");
    std::io::Read::read_to_end(&mut file, &mut bytes).expect("read assets.bin");
    let db = assets_bin::parse_assets_bin(&bytes).expect("db");
    let self_id_index = db.build_self_id_index();
    let metadata = GameMetadataProvider::from_vfs(&vfs).expect("GameParams");

    let started = std::time::Instant::now();
    let index = ModelPathIndex::build(&db, &self_id_index);
    let indexed = started.elapsed();
    let links = CatalogLinks::from_metadata(&metadata);
    let linked = started.elapsed();
    let catalog = ModelCatalog::from_paths(index.paths().cloned(), &links);
    eprintln!(
        "[measured] index {indexed:?}, GameParams links {:?}, catalogue {:?}",
        linked - indexed,
        started.elapsed() - linked
    );

    Some(Install { index, links, catalog })
});

fn path(raw: &str) -> ModelPath {
    ModelPath::parse(raw).expect("valid model path")
}

#[test]
#[ignore = "requires a World of Warships install"]
fn catalogue_groups_every_model() {
    let Some(Install { index, links, catalog }) = INSTALL.as_ref() else {
        return;
    };

    let listed = catalog.listed_entries().count();
    eprintln!(
        "{} .model prototypes, {} base models ({listed} listed, {} skins), {} peculiarity pairs, {} building params",
        index.len(),
        catalog.entries().len(),
        catalog.entries().len() - listed,
        links.skins().len(),
        links.buildings().len()
    );
    let counts = catalog.count_by_group();
    for kind in ModelGroupKind::ALL {
        let count = counts.get(&kind).copied().unwrap_or(0);
        eprintln!("  {kind:?}: {count}");
        assert!(count > 0, "{kind:?} models are catalogued");
    }

    // Every model is a base entry or a dead or section variant of exactly one.
    let unowned: Vec<&ModelPath> = index.paths().filter(|p| catalog.owning_entry(p).is_none()).collect();
    assert!(unowned.is_empty(), "models outside the catalogue: {unowned:?}");
    let mut seen: HashSet<&ModelPath> = HashSet::new();
    for entry in catalog.entries() {
        let non_skins = entry.variants().iter().filter(|v| v.kind != ModelVariantKind::Skin).map(|v| &v.path);
        for owned in std::iter::once(entry.path()).chain(non_skins) {
            assert!(seen.insert(owned), "{owned} belongs to more than one entry");
        }
    }
    assert_eq!(seen.len(), index.len());

    let iowa = catalog
        .entry(&path("content/gameplay/usa/ship/battleship/ASB028_Iowa_1945/ASB028_Iowa_1945.model"))
        .expect("Iowa is a base model");
    let sections: Vec<&str> = iowa.sections().map(ModelPath::stem).collect();
    assert_eq!(
        sections,
        ["ASB028_Iowa_1945_Bow", "ASB028_Iowa_1945_MidBack", "ASB028_Iowa_1945_MidFront", "ASB028_Iowa_1945_Stern"]
    );

    let turret = catalog
        .entry(&path("content/gameplay/usa/gun/main/AGM034_16in50_Mk7/AGM034_16in50_Mk7.model"))
        .expect("Iowa main turret");
    assert!(turret.variants().iter().any(|v| v.kind == ModelVariantKind::Dead), "the turret has a wreck model");

    let skinned_stock =
        catalog.entries().iter().filter(|e| e.variants().iter().any(|v| v.kind == ModelVariantKind::Skin));
    assert!(skinned_stock.count() > 10, "GameParams links skins to their stock models");
    assert!(
        catalog
            .entries()
            .iter()
            .any(|e| !e.skin_of().is_empty() && e.group().gameplay().is_some_and(|g| g.kind() == ModelKind::Ship)),
        "some ship skins nest under their stock hull"
    );

    let gameplay_kinds: BTreeMap<ModelKind, usize> =
        catalog.entries().iter().filter_map(|e| e.group().gameplay()).fold(BTreeMap::new(), |mut counts, group| {
            *counts.entry(group.kind()).or_insert(0) += 1;
            counts
        });
    for kind in ModelKind::ALL {
        assert!(gameplay_kinds.get(&kind).copied().unwrap_or(0) > 0, "{kind:?} gameplay models are catalogued");
    }

    let mut scenes: BTreeMap<String, usize> = BTreeMap::new();
    for entry in catalog.entries() {
        let label = match entry.group() {
            ModelGroup::Location(scene) => format!("location/{}", scene.category()),
            ModelGroup::Port(scene) => format!("ports/{}", scene.category()),
            ModelGroup::Style(style) => format!("styles/{style}"),
            ModelGroup::Other(directory) => format!("other/{directory:?}"),
            ModelGroup::Character(nation) => format!("character/{nation}"),
            ModelGroup::Gameplay(_) | ModelGroup::Building(_) => continue,
        };
        *scenes.entry(label).or_insert(0) += 1;
    }
    eprintln!("{scenes:#?}");
}

#[test]
#[ignore = "requires a World of Warships install"]
fn buildings_group_by_their_game_params() {
    let Some(Install { catalog, .. }) = INSTALL.as_ref() else {
        return;
    };

    let mut kinds: BTreeMap<&BuildingKind, Vec<&str>> = BTreeMap::new();
    for entry in catalog.entries() {
        if let ModelGroup::Building(kind) = entry.group() {
            kinds.entry(kind).or_default().push(entry.name());
        }
    }
    for (kind, names) in &kinds {
        eprintln!("  {kind:?}: {} e.g. {:?}", names.len(), &names[..names.len().min(4)]);
    }

    let entry = |raw: &str| catalog.entry(&path(raw)).unwrap_or_else(|| panic!("{raw} is a base model"));
    let fort = entry("content/location/gameplay/building/LYB015/LYB015.model");
    assert_eq!(fort.group(), &ModelGroup::Building(BuildingKind::Complex));
    assert!(fort.building_params().iter().any(|p| p.name() == "PCBX001"), "{:?}", fort.building_params());

    let airfield = entry("content/location/nature/gameplay/LNG001/LNG001.model");
    assert_eq!(airfield.group(), &ModelGroup::Building(BuildingKind::AirBase));
    assert!(airfield.variants().iter().any(|v| v.kind == ModelVariantKind::Dead), "the aerodrome has a wreck");

    let field_gun = entry("content/location/gameplay/gun/main/LYGM002_105mm_LeFH_18/LYGM002_105mm_LeFH_18.model");
    assert_eq!(field_gun.group(), &ModelGroup::Building(BuildingKind::Weapon));

    let tower = entry("content/location/gameplay/building/LYB048_PA_Building/LYB048_PA_Building.model");
    assert_eq!(tower.group(), &ModelGroup::Building(BuildingKind::RayTower));
    let wreck = path("content/location/gameplay/building/LYB048_PA_Building_dead/LYB048_PA_Building_dead.model");
    assert_eq!(catalog.owning_entry(&wreck).map(|e| e.path()), Some(tower.path()), "deadModel links the wreck");

    // A ship gun a fort mounts stays in its gameplay folder and names the fort.
    let fort_gun = entry("content/gameplay/germany/gun/main/GGM058_280mm54_5_C34/GGM058_280mm54_5_C34.model");
    assert!(fort_gun.group().gameplay().is_some());
    let param_kinds: Vec<&BuildingKind> = fort_gun.building_params().iter().map(BuildingParamRef::kind).collect();
    assert!(param_kinds.contains(&&BuildingKind::Complex), "{param_kinds:?}");
}

#[test]
#[ignore = "requires a World of Warships install"]
fn misc_models_fall_into_named_categories() {
    let Some(Install { catalog, .. }) = INSTALL.as_ref() else {
        return;
    };

    let mut categories: BTreeMap<MiscCategory, Vec<&str>> = BTreeMap::new();
    for entry in catalog.entries() {
        let Some(group) = entry.group().gameplay() else {
            continue;
        };
        if group.kind() != ModelKind::Misc {
            continue;
        }
        let Some(ModelSubKind::Misc(category)) = group.sub_kind() else {
            panic!("{} is a misc model without a category", entry.path());
        };
        categories.entry(*category).or_default().push(entry.name());
    }

    let total: usize = categories.values().map(Vec::len).sum();
    for category in MiscCategory::ALL {
        let names = categories.get(&category).map_or(&[][..], Vec::as_slice);
        let step = (names.len() / 5).max(1);
        let sample: Vec<&&str> = names.iter().step_by(step).take(5).collect();
        eprintln!("  {category:?}: {} e.g. {sample:?}", names.len());
        assert!(!names.is_empty(), "{category:?} holds misc models");
    }
    let other = categories.get(&MiscCategory::Other).map_or(0, Vec::len);
    assert!(other * 4 < total, "{other} of {total} misc models are unclassified");
}

/// Availability matches what a load finds: a model marked present loads its
/// geometry, and one marked absent fails for that reason.
#[test]
#[ignore = "requires a World of Warships install"]
fn geometry_availability_matches_loads() {
    let dir =
        std::env::var_os("WOWS_DIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"E:\WoWs\World_of_Warships"));
    if !dir.join("bin").is_dir() {
        eprintln!("skipped: no install at {}", dir.display());
        return;
    }
    let assets = ShipAssets::from_game_dir(&dir).expect("ship assets");
    let started = std::time::Instant::now();
    let library = assets.model_library().expect("model library");
    eprintln!("[measured] model library with geometry availability built in {:?}", started.elapsed());
    let catalog = library.catalog();

    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for entry in catalog.entries() {
        *counts.entry(format!("{:?}", catalog.geometry(entry.path()))).or_insert(0) += 1;
    }
    eprintln!("{counts:#?}");
    assert!(catalog.entries().iter().all(|entry| catalog.geometry(entry.path()).is_some()), "every entry is resolved");

    let options = ModelLoadOptions { textures: TextureDecode::None, ..ModelLoadOptions::default() };
    let mut checked = 0usize;
    for entry in catalog.entries().iter().step_by(97) {
        let expected = catalog.geometry(entry.path()).expect("resolved");
        let outcome = library.load_model(entry.path(), &options);
        match expected {
            GeometryAvailability::Present => {
                if let Err(error) = &outcome {
                    let context = error.current_context();
                    assert!(
                        !matches!(context, ModelLoadError::GeometryAbsent { .. } | ModelLoadError::NoGeometry { .. }),
                        "{} is marked present: {error}",
                        entry.path()
                    );
                }
            }
            GeometryAvailability::Absent => {
                let error = outcome.err().unwrap_or_else(|| panic!("{} is marked absent but loads", entry.path()));
                assert!(matches!(error.current_context(), ModelLoadError::GeometryAbsent { .. }), "{error}");
            }
            GeometryAvailability::NoGeometry => {
                let error = outcome.err().unwrap_or_else(|| panic!("{} has no geometry but loads", entry.path()));
                assert!(matches!(error.current_context(), ModelLoadError::NoGeometry { .. }), "{error}");
            }
        }
        checked += 1;
    }
    eprintln!("{checked} sampled loads agree");
    let fort = path("content/location/gameplay/building/LYB015/LYB015.model");
    assert_eq!(catalog.geometry(&fort), Some(GeometryAvailability::Absent));
}
