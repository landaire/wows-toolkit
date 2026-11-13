//! The textures a real model's materials reference and the files holding them. Ignored: needs an
//! install named by `WOWS_DIR`; returns early when that directory is absent.

use std::collections::BTreeMap;
use std::fmt::Write;
use std::path::PathBuf;

use wowsunpack::export::material_textures::TextureReference;
use wowsunpack::export::material_textures::texture_files;
use wowsunpack::export::model_catalog::ModelPath;
use wowsunpack::export::model_library::ModelLoadOptions;
use wowsunpack::export::model_library::TextureDecode;
use wowsunpack::export::ship::ShipAssets;

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

#[test]
#[ignore = "requires a World of Warships install"]
fn iowa_turret_materials_list_their_texture_files() {
    let Some(assets) = assets() else { return };
    let library = assets.model_library().expect("model library");
    let options = ModelLoadOptions { textures: TextureDecode::None, ..ModelLoadOptions::default() };
    let turret = ModelPath::parse(IOWA_TURRET).expect("model path");
    let model = library.load_model(&turret, &options).expect("the turret loads");

    let materials: BTreeMap<&str, u64> =
        model.meshes().iter().filter_map(|mesh| Some((mesh.mfm_path.as_deref()?, mesh.mfm_path_id))).collect();

    let mut listing = String::new();
    for (mfm_path, mfm_path_id) in materials {
        writeln!(listing, "{mfm_path}").unwrap();
        let textures = library.material_textures(mfm_path_id).expect("the material record reads");
        for texture in textures {
            let path = match texture.texture {
                TextureReference::Path(path) => path,
                TextureReference::Unresolved(path_id) => format!("unresolved 0x{path_id:016X}"),
            };
            writeln!(listing, "  {}: {path}", texture.property).unwrap();
            let Ok(files) = texture_files(library.vfs(), &path) else {
                writeln!(listing, "    not in the packages").unwrap();
                continue;
            };
            let format = files.format.map_or_else(|| "unrecognized".to_string(), |format| format.to_string());
            writeln!(listing, "    {format}, {} mips", files.mip_count).unwrap();
            for file in &files.files {
                writeln!(listing, "    {} {}", file.path, file.size).unwrap();
            }
        }
    }

    insta::assert_snapshot!(listing);
}
