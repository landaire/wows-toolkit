//! Normal and metal-gloss maps over a real install: decoded for the viewer
//! and embedded in GLB exports in glTF's channel layout. Ignored: needs an
//! install named by `WOWS_DIR`; returns early when that directory is absent.

use std::path::PathBuf;
use std::time::Instant;

use wowsunpack::export::model_catalog::ModelPath;
use wowsunpack::export::model_library::ModelArmorMaps;
use wowsunpack::export::model_library::ModelExportOptions;
use wowsunpack::export::ship::ExportContents;
use wowsunpack::export::ship::ShipAssets;
use wowsunpack::export::ship::ShipExportOptions;
use wowsunpack::export::surface_maps::SurfaceMaps;
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

fn small_textures() -> TextureLod {
    TextureLod::Capped(MaxEdge::new(256).expect("nonzero"))
}

fn iowa(assets: &ShipAssets, options: &ShipExportOptions) -> wowsunpack::export::ship::ShipModelContext {
    let vehicle = assets
        .metadata()
        .params()
        .iter()
        .filter_map(|param| param.vehicle())
        .find(|vehicle| vehicle.model_path().is_some_and(|path| path.contains(IOWA_DIR)))
        .expect("Iowa in GameParams")
        .clone();

    assets.load_ship_from_vehicle(&vehicle, options).expect("Iowa loads")
}

/// Mean of one channel over an RGBA8 buffer.
fn channel_mean(pixels: &[u8], channel: usize) -> f64 {
    let texels = pixels.chunks_exact(4);
    let count = texels.len().max(1) as f64;

    texels.map(|texel| f64::from(texel[channel])).sum::<f64>() / count
}

/// The decoded RGBA8 pixels of a texture embedded in a GLB.
fn embedded_pixels(gltf: &gltf::Gltf, texture: gltf::Texture<'_>) -> Vec<u8> {
    let gltf::image::Source::View { view, .. } = texture.source().source() else {
        panic!("textures are embedded");
    };
    let blob = gltf.blob.as_deref().expect("a binary chunk");
    let png = &blob[view.offset()..view.offset() + view.length()];

    image::load_from_memory(png).expect("the PNG decodes").into_rgba8().into_raw()
}

/// Asserts that `gltf` has a material with both surface maps, and that they
/// hold glTF's channel layout: a unit normal pointing out of the surface,
/// and the occlusion channel of the metallic-roughness map unused.
fn assert_gltf_surface_maps(gltf: &gltf::Gltf) {
    let material = gltf
        .document
        .materials()
        .find(|material| {
            material.normal_texture().is_some()
                && material.pbr_metallic_roughness().metallic_roughness_texture().is_some()
        })
        .expect("a material carries both surface maps");

    let normal = embedded_pixels(gltf, material.normal_texture().expect("normal").texture());
    let [x, y, z] = [0, 1, 2].map(|channel| channel_mean(&normal, channel) / 255.0 * 2.0 - 1.0);
    eprintln!("mean normal ({x:.3}, {y:.3}, {z:.3})");
    assert!(z > 0.8, "Z is rebuilt toward the viewer: {z}");
    assert!(x.abs() < 0.2 && y.abs() < 0.2, "X and Y center on zero: {x} {y}");

    let metal_roughness =
        embedded_pixels(gltf, material.pbr_metallic_roughness().metallic_roughness_texture().expect("mr").texture());
    assert!(metal_roughness.chunks_exact(4).all(|texel| texel[0] == u8::MAX && texel[3] == u8::MAX));
    eprintln!(
        "mean roughness {:.1}, metalness {:.1}",
        channel_mean(&metal_roughness, 1),
        channel_mean(&metal_roughness, 2)
    );
}

#[test]
#[ignore = "requires a World of Warships install"]
fn iowa_hull_materials_name_normal_and_metal_gloss_maps() {
    let Some(assets) = assets() else {
        return;
    };
    let ship = iowa(&assets, &ShipExportOptions { textures: false, ..ShipExportOptions::default() });
    let meshes = ship.interactive_hull_meshes().expect("hull meshes");
    let reader = ship.material_reader();
    let session = reader.open().expect("assets.bin parses");
    let mut materials: Vec<(&String, u64)> =
        meshes.iter().filter_map(|mesh| Some((mesh.mfm_path.as_ref()?, mesh.mfm_path_id))).collect();
    materials.sort();
    materials.dedup();

    let started = Instant::now();
    let maps: Vec<(&String, SurfaceMaps)> =
        materials.iter().map(|&(path, id)| (path, session.surface_maps(id, small_textures()))).collect();
    eprintln!("[measured] Iowa surface maps: {:?} for {} materials", started.elapsed(), maps.len());

    let both = maps.iter().filter(|(_, maps)| maps.normal.is_some() && maps.metal_gloss.is_some()).count();
    assert!(both > 0, "a hull material names both maps");
    let hull = maps
        .iter()
        .find(|(path, maps)| path.contains("Iowa") && maps.normal.is_some())
        .map(|(_, maps)| maps)
        .expect("the hull's own material has a normal map");
    let normal = hull.normal.as_ref().expect("normal").image();
    for channel in [0, 1] {
        let mean = channel_mean(normal.as_raw(), channel);
        assert!((108.0..148.0).contains(&mean), "channel {channel} centers on zero: {mean}");
    }
}

#[test]
#[ignore = "requires a World of Warships install"]
fn a_ship_glb_carries_normal_and_metallic_roughness_textures() {
    let Some(assets) = assets() else {
        return;
    };
    let options = ShipExportOptions {
        textures: true,
        texture_lod: small_textures(),
        contents: ExportContents::Mesh,
        ..ShipExportOptions::default()
    };
    let ship = iowa(&assets, &options);
    let mut glb = Vec::new();
    ship.export_glb(&mut glb).expect("export");

    let gltf = gltf::Gltf::from_slice(&glb).expect("a valid GLB");
    assert_gltf_surface_maps(&gltf);
}

#[test]
#[ignore = "requires a World of Warships install"]
fn a_model_glb_carries_normal_and_metallic_roughness_textures() {
    let Some(assets) = assets() else {
        return;
    };
    let library = assets.model_library().expect("model library");
    let turret = ModelPath::parse(IOWA_TURRET).expect("model path");
    let options = ModelExportOptions {
        lod: 0,
        damaged: false,
        texture_lod: small_textures(),
        textures: true,
        contents: ExportContents::Mesh,
        hardpoints: false,
    };
    let mut glb = Vec::new();
    library.export_glb(&turret, &options, ModelArmorMaps::default(), &mut glb).expect("export");

    let gltf = gltf::Gltf::from_slice(&glb).expect("a valid GLB");
    assert_gltf_surface_maps(&gltf);

    let plain = ModelExportOptions { textures: false, ..options };
    let mut glb = Vec::new();
    library.export_glb(&turret, &plain, ModelArmorMaps::default(), &mut glb).expect("export");
    let gltf = gltf::Gltf::from_slice(&glb).expect("a valid GLB");
    assert!(
        gltf.document.materials().all(|material| material.normal_texture().is_none()),
        "an export without textures has no surface maps"
    );
}
