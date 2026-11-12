//! The model catalogue over a real install. Ignored: needs an install named by
//! `WOWS_DIR`; returns early when that directory is absent.

use std::path::PathBuf;

use wowsunpack::export::model_catalog;
use wowsunpack::export::model_catalog::ModelCatalog;
use wowsunpack::export::model_catalog::ModelKind;
use wowsunpack::export::model_catalog::ModelPath;
use wowsunpack::export::model_catalog::ModelPathIndex;
use wowsunpack::export::model_catalog::ModelVariantKind;
use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::models::assets_bin;

fn game_dir() -> Option<PathBuf> {
    let dir =
        std::env::var_os("WOWS_DIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"E:\WoWs\World_of_Warships"));
    if dir.join("bin").is_dir() {
        Some(dir)
    } else {
        eprintln!("skipped: no install at {}", dir.display());
        None
    }
}

fn path(raw: &str) -> ModelPath {
    ModelPath::parse(raw).expect("valid model path")
}

#[test]
#[ignore = "requires a World of Warships install"]
fn catalogue_groups_every_gameplay_model() {
    let Some(dir) = game_dir() else {
        return;
    };
    let vfs = wowsunpack::game_data::build_game_vfs(&dir, wowsunpack::game_data::AssetsBin::Omit).expect("game vfs");
    let mut bytes = Vec::new();
    let mut file = vfs.join("content/assets.bin").expect("assets.bin path").open_file().expect("open assets.bin");
    std::io::Read::read_to_end(&mut file, &mut bytes).expect("read assets.bin");
    let db = assets_bin::parse_assets_bin(&bytes).expect("db");
    let self_id_index = db.build_self_id_index();
    let metadata = GameMetadataProvider::from_vfs(&vfs).expect("GameParams");

    let started = std::time::Instant::now();
    let index = ModelPathIndex::build(&db, &self_id_index);
    let skins = model_catalog::peculiarity_models(&metadata);
    let catalog = ModelCatalog::from_paths(index.paths().cloned(), &skins);
    eprintln!("[measured] index and catalogue built in {:?}", started.elapsed());

    let listed = catalog.listed_entries().count();
    let skin_count = catalog.entries().len() - listed;
    eprintln!(
        "{} .model prototypes, {} gameplay base models ({listed} listed, {skin_count} skins), {} peculiarity pairs",
        index.len(),
        catalog.entries().len(),
        skins.len()
    );
    for (kind, count) in catalog.count_by_kind() {
        eprintln!("  {kind:?}: {count}");
    }
    let counts = catalog.count_by_kind();
    for kind in ModelKind::ALL {
        assert!(counts.get(&kind).copied().unwrap_or(0) > 0, "{kind:?} models are catalogued");
    }

    // Every gameplay model is a base entry or a variant of exactly one.
    let gameplay: Vec<&ModelPath> = index.paths().filter(|p| p.as_str().starts_with("content/gameplay/")).collect();
    let unowned: Vec<&&ModelPath> = gameplay.iter().filter(|p| catalog.owning_entry(p).is_none()).collect();
    assert!(unowned.is_empty(), "gameplay models outside the catalogue: {unowned:?}");
    let variant_count: usize =
        catalog.entries().iter().flat_map(|e| e.variants()).filter(|v| v.kind != ModelVariantKind::Skin).count();
    assert_eq!(catalog.entries().len() + variant_count, gameplay.len());

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
        catalog.entries().iter().any(|e| !e.skin_of().is_empty() && e.kind() == ModelKind::Ship),
        "some ship skins nest under their stock hull"
    );
}
