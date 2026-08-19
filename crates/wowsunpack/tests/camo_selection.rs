//! Selected camos, and only those, reach the GLB. Ignored: needs an install.

use std::collections::HashMap;
use std::path::PathBuf;

use wowsunpack::export::camo_composite;
use wowsunpack::export::camo_textures::CamoSchemeId;
use wowsunpack::export::ship::CamoSelection;
use wowsunpack::export::ship::ShipAssets;
use wowsunpack::export::ship::ShipExportOptions;
use wowsunpack::export::ship::ShipModelContext;
use wowsunpack::game_params::types::GameParamProvider;

fn game_dir() -> PathBuf {
    std::env::var_os("WOWS_DIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"E:\WoWs\World_of_Warships"))
}

fn load(options: &ShipExportOptions) -> ShipModelContext {
    load_ship("WSD011_Smaland_1955", options)
}

fn load_ship(model_dir: &str, options: &ShipExportOptions) -> ShipModelContext {
    let assets = ShipAssets::from_game_dir(&game_dir()).expect("assets");
    let vehicle = assets
        .metadata()
        .params()
        .iter()
        .filter_map(|p| p.vehicle())
        .find(|v| v.model_path().map(|mp| mp.contains(model_dir)).unwrap_or(false))
        .cloned()
        .unwrap_or_else(|| panic!("no vehicle for {model_dir}"));
    assets.load_ship_from_vehicle(&vehicle, options).expect("ctx")
}

/// The `KHR_materials_variants` names declared at the glTF root.
fn variant_names(glb: &[u8]) -> Vec<String> {
    let g = gltf::Gltf::from_slice(glb).expect("parse glb");
    let json = g.document.into_json();
    let raw = serde_json::to_value(&json).expect("to json value");
    raw.pointer("/extensions/KHR_materials_variants/variants")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|e| e.get("name")?.as_str().map(str::to_string)).collect())
        .unwrap_or_default()
}

/// Whether any material's base color texture carries a `KHR_texture_transform`
/// with a non-identity scale.
fn any_material_has_non_identity_texture_transform(glb: &[u8]) -> bool {
    let g = gltf::Gltf::from_slice(glb).expect("parse glb");
    let json = g.document.into_json();
    let raw = serde_json::to_value(&json).expect("to json value");
    let Some(materials) = raw.pointer("/materials").and_then(|v| v.as_array()) else {
        return false;
    };
    materials.iter().any(|m| {
        let Some(scale) = m
            .pointer("/pbrMetallicRoughness/baseColorTexture/extensions/KHR_texture_transform/scale")
            .and_then(|v| v.as_array())
        else {
            return false;
        };
        let scale: Vec<f64> = scale.iter().filter_map(|v| v.as_f64()).collect();
        scale.len() == 2 && (scale[0] != 1.0 || scale[1] != 1.0)
    })
}

/// Decode a stem-to-PNG map into raw RGBA, mirroring `ship.rs`'s private
/// `decode_png_map` (not reachable from an integration test).
fn decode_png_map(pngs: &HashMap<String, Vec<u8>>) -> HashMap<String, camo_composite::RgbaImageData> {
    let mut out = HashMap::new();
    for (stem, png) in pngs {
        let Ok(img) = image::load_from_memory(png) else { continue };
        let rgba = img.to_rgba8();
        let (width, height) = (rgba.width(), rgba.height());
        out.insert(stem.clone(), camo_composite::RgbaImageData { width, height, pixels: rgba.into_raw() });
    }
    out
}

/// A scheme only carries tiling into a baked base material through the opaque
/// `Replace` branch; the coverage (`CompositeOverBase`) and recolor branches bake
/// tiling into pixels and correctly report identity UVs. Selecting on the raw
/// `uv_transforms` map alone (as an earlier version of this test did) can pick a
/// scheme that never reaches `Replace`, so classify for real via `apply_scheme`.
///
/// Bounded to at most `max_examined` candidates (schemes with a non-empty
/// `uv_transforms`) because each candidate requires a real decode. Returns the
/// found id, if any, and how many candidates were actually examined.
fn first_scheme_baking_to_a_tiled_replace(
    source: &wowsunpack::export::camo_textures::CamoTextureSource,
    bases: &HashMap<String, camo_composite::RgbaImageData>,
    max_examined: usize,
    examined_so_far: &mut usize,
) -> Option<CamoSchemeId> {
    for info in source.scheme_infos() {
        if info.uv_transforms.is_empty() {
            continue;
        }
        if *examined_so_far >= max_examined {
            return None;
        }
        *examined_so_far += 1;

        let Ok(textures) = source.decode(info.id) else { continue };
        let applied = camo_composite::apply_scheme(&textures, &info.uv_transforms, info.use_color_scheme, bases);
        let tiled_replace = applied.values().any(|app| {
            matches!(
                app,
                camo_composite::CamoApplication::Replace { uv, .. }
                    if uv.scale != [1.0, 1.0] || uv.offset != [0.0, 0.0]
            )
        });
        if tiled_replace {
            return Some(info.id);
        }
    }
    None
}

#[test]
#[ignore = "requires a World of Warships install"]
fn base_only_emits_no_variants() {
    let options = ShipExportOptions { camos: CamoSelection::BaseOnly, ..Default::default() };
    let mut out = Vec::new();
    load(&options).export_glb(&mut out).expect("export");
    assert!(variant_names(&out).is_empty(), "base-only emits no variants extension");
}

#[test]
#[ignore = "requires a World of Warships install"]
fn variants_emits_exactly_the_selected_ids() {
    let probe = load(&ShipExportOptions { textures: false, ..Default::default() });
    let source = probe.camo_texture_source().expect("source");
    let infos = source.scheme_infos();
    assert!(infos.len() >= 5, "Smaland should offer several schemes");

    // Non-adjacent ids, so a shifted enumeration cannot pass by accident.
    let picked = vec![infos[0].id, infos[3].id];
    let expected: Vec<String> =
        picked.iter().map(|id| infos.iter().find(|i| i.id == *id).unwrap().display_name.clone()).collect();

    let options = ShipExportOptions { camos: CamoSelection::Variants(picked), ..Default::default() };
    let mut out = Vec::new();
    load(&options).export_glb(&mut out).expect("export");

    let mut got = variant_names(&out);
    let mut want = expected;
    got.sort();
    want.sort();
    assert_eq!(got, want, "exactly the selected schemes reach the file");
}

#[test]
#[ignore = "requires a World of Warships install"]
fn an_unknown_id_is_an_error_not_a_silent_drop() {
    let options = ShipExportOptions { camos: CamoSelection::Variants(vec![CamoSchemeId(9999)]), ..Default::default() };
    let mut out = Vec::new();
    let result = load(&options).export_glb(&mut out);
    assert!(result.is_err(), "an id that names no scheme is a caller bug, not a silent empty export");
}

#[test]
#[ignore = "requires a World of Warships install"]
fn baking_emits_no_variants_and_changes_the_base() {
    let probe = load(&ShipExportOptions { textures: false, ..Default::default() });
    let source = probe.camo_texture_source().expect("source");
    let id = source.scheme_infos().first().expect("at least one scheme").id;

    let mut stock = Vec::new();
    load(&ShipExportOptions { camos: CamoSelection::BaseOnly, ..Default::default() })
        .export_glb(&mut stock)
        .expect("stock");

    let mut baked = Vec::new();
    load(&ShipExportOptions { camos: CamoSelection::Baked(id), ..Default::default() })
        .export_glb(&mut baked)
        .expect("baked");

    assert!(variant_names(&baked).is_empty(), "a baked camo needs no variants extension");
    assert_ne!(baked, stock, "baking changes the base material's textures");
}

#[test]
#[ignore = "requires a World of Warships install"]
fn baking_stays_in_the_same_size_class_as_stock() {
    let probe = load(&ShipExportOptions { textures: false, ..Default::default() });
    let source = probe.camo_texture_source().expect("source");
    let id = source.scheme_infos().first().expect("at least one scheme").id;

    let mut stock = Vec::new();
    load(&ShipExportOptions { camos: CamoSelection::BaseOnly, ..Default::default() })
        .export_glb(&mut stock)
        .expect("stock");
    let mut baked = Vec::new();
    load(&ShipExportOptions { camos: CamoSelection::Baked(id), ..Default::default() })
        .export_glb(&mut baked)
        .expect("baked");

    // Baking replaces each stem's albedo rather than adding a texture set, so it
    // stays near stock. Compositing per stem loses the cross-stem image dedup a
    // shared camo mask enjoys, so it can exceed stock; it must not approach the
    // multiple that carrying a variant would cost.
    assert!(baked.len() < stock.len() * 2, "baked {} bytes against stock {} bytes", baked.len(), stock.len());
}

#[test]
#[ignore = "requires a World of Warships install"]
fn baking_a_tiled_camo_keeps_its_transform_on_the_default_material() {
    let ship_names = ["WSD011_Smaland_1955", "JSB039_Yamato_1945"];
    const MAX_EXAMINED: usize = 20;
    let mut examined = 0usize;
    let mut found: Option<(&str, CamoSchemeId)> = None;

    for name in ship_names {
        if examined >= MAX_EXAMINED {
            break;
        }
        let probe = load_ship(name, &ShipExportOptions { textures: false, ..Default::default() });
        let source = probe.camo_texture_source().expect("source");
        let bases = decode_png_map(&source.base_albedos());

        if let Some(id) = first_scheme_baking_to_a_tiled_replace(&source, &bases, MAX_EXAMINED, &mut examined) {
            found = Some((name, id));
            break;
        }
    }

    let Some((name, id)) = found else {
        eprintln!(
            "no scheme among the first {examined} candidate(s) with a non-empty uv_transforms map, \
             across {ship_names:?}, classifies as a tiled Replace application (apply_scheme always routed \
             them through CompositeOverBase or the recolor path instead); base_uv_transforms may be a code \
             path real game data never exercises"
        );
        return;
    };

    let mut baked = Vec::new();
    load_ship(name, &ShipExportOptions { camos: CamoSelection::Baked(id), ..Default::default() })
        .export_glb(&mut baked)
        .expect("baked");

    assert!(
        any_material_has_non_identity_texture_transform(&baked),
        "a baked tiled-Replace camo ({id:?} on {name}) must carry KHR_texture_transform on its default material"
    );
}
