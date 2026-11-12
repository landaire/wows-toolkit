//! The normal and metal-gloss maps of a scene's hull materials, decoded off
//! the UI thread and ready to upload beside their albedos.

use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use wowsunpack::export::gltf_export::InteractiveHullMesh;
use wowsunpack::export::model_library::MaterialTexture;
use wowsunpack::export::model_library::ModelMaterial;
use wowsunpack::export::ship::ShipModelContext;
use wowsunpack::export::surface_maps::SurfaceMaps;
use wowsunpack::export::texture::TextureLod;

use super::camo::RgbaTexture;
use super::scene_load::LoadCancel;
use super::scene_load::LoadCancelled;

/// One material's surface maps, as stored: normal X and Y in R and G, gloss
/// in R and metalness in G of the metal-gloss map.
#[derive(Debug)]
pub(crate) struct SurfaceTextures {
    pub(crate) normal: Option<RgbaTexture>,
    pub(crate) metal_gloss: Option<RgbaTexture>,
}

impl SurfaceTextures {
    /// `None` when `maps` holds neither map.
    fn from_maps(maps: SurfaceMaps) -> Option<Self> {
        let texture = |image: image::RgbaImage| RgbaTexture::new(image.width(), image.height(), image.into_raw());
        let normal = maps.normal.map(|map| texture(map.into_image()));
        let metal_gloss = maps.metal_gloss.map(|map| texture(map.into_image()));

        (normal.is_some() || metal_gloss.is_some()).then_some(Self { normal, metal_gloss })
    }
}

/// Surface maps by `.mfm` path, as hull albedos are keyed.
pub(crate) type SurfaceMapsByPart = HashMap<String, SurfaceTextures>;

/// Decodes the surface maps of each material `hull_meshes` names, once per
/// material and on several threads, stopping early when `cancel` is set.
pub(crate) fn decode_ship_surface_maps(
    ctx: &ShipModelContext,
    hull_meshes: &[InteractiveHullMesh],
    lod: TextureLod,
    cancel: &LoadCancel,
) -> Result<SurfaceMapsByPart, LoadCancelled> {
    // Decoding is mostly file reads behind one archive and BC decode; past a
    // few threads the reads serialize.
    const MOST_WORKERS: usize = 8;

    let mut seen = HashSet::new();
    let materials: Vec<(&str, u64)> = hull_meshes
        .iter()
        .filter_map(|mesh| mesh.mfm_path.as_deref().map(|path| (path, mesh.mfm_path_id)))
        .filter(|(path, _)| seen.insert(*path))
        .collect();

    let reader = ctx.material_reader();
    let session = match reader.open() {
        Ok(session) => session,
        Err(report) => {
            tracing::warn!(
                "armor viewer: the hull materials cannot be read, so the hull draws without surface maps: {report}"
            );
            return Ok(SurfaceMapsByPart::new());
        }
    };

    let half_the_cores = std::thread::available_parallelism().map_or(1, |cores| cores.get() / 2);
    let workers = half_the_cores.clamp(1, MOST_WORKERS).min(materials.len());
    let next = AtomicUsize::new(0);
    let decoded: Vec<Vec<(String, SurfaceTextures)>> = std::thread::scope(|scope| {
        let decode = || {
            let mut out = Vec::new();
            while let Some(&(path, mfm_path_id)) = materials.get(next.fetch_add(1, Ordering::Relaxed)) {
                if cancel.is_cancelled() {
                    break;
                }
                let Some(textures) = SurfaceTextures::from_maps(session.surface_maps(mfm_path_id, lod)) else {
                    continue;
                };
                out.push((path.to_string(), textures));
            }

            out
        };
        let handles: Vec<_> = (0..workers).map(|_| scope.spawn(decode)).collect();

        handles
            .into_iter()
            .map(|handle| handle.join().unwrap_or_else(|panic| std::panic::resume_unwind(panic)))
            .collect()
    });
    cancel.checkpoint()?;

    Ok(decoded.into_iter().flatten().collect())
}

/// Takes the decoded surface maps out of `material`. `None` when neither was
/// decoded, which needs `TextureDecode::AllMaps`.
pub(crate) fn model_surface_textures(material: &mut ModelMaterial) -> Option<SurfaceTextures> {
    let texture = |map: Option<MaterialTexture>| {
        let image = map?.image?;

        Some(RgbaTexture::new(image.width, image.height, image.pixels))
    };
    let normal = texture(material.normal.take());
    let metal_gloss = texture(material.metal_gloss.take());

    (normal.is_some() || metal_gloss.is_some()).then_some(SurfaceTextures { normal, metal_gloss })
}
