//! Uploads a [`LoadedShipArmor`]'s hull meshes into a [`Viewport3D`],
//! separately from the armor upload (`upload.rs`) so a hull-only visibility
//! or opacity change never rebuilds the (typically much larger) armor mesh
//! set. Ports the egui app's `upload_hull_meshes_to_viewport`
//! (`armor_viewer/ui/tab.rs:1339-1442`).
//!
//! **Camo compositing.** The active camo's decoded
//! textures/UV transforms are not carried on `armor` (an `Arc<LoadedShipArmor>`
//! is never mutated once loaded): the host owns that state and passes it in
//! as parameters here on every hull upload. A stem present in
//! `active_camo_textures` takes precedence over `armor.hull_textures`'s base
//! albedo for that mesh; empty maps (no camo selected) reproduce Task 8a's
//! base-albedo-only behavior exactly.
//!
//! Hull-upgrade/LOD reload (Task 8c) is out of scope; this always uploads
//! whatever `armor.hull_meshes` currently holds. The meshes are uploaded
//! non-pickable: hover picking walks `armor.hull_pick` (`hull_picking.rs`)
//! instead, so shell casts through the armor never stop on the hull.

use std::collections::HashMap;

use wowsunpack::export::armor_ray::transform_point;
use wowsunpack::export::camouflage::UvTransform;
use wowsunpack::export::gltf_export::InteractiveHullMesh;
use wowsunpack::export::material_alpha::AlphaMode;

use crate::viewport::renderer::AlbedoUv;
use crate::viewport::renderer::GpuPipeline;
use crate::viewport::renderer::LAYER_DEFAULT;
use crate::viewport::renderer::LAYER_HULL;
use crate::viewport::renderer::MaterialBinding;
use crate::viewport::renderer::TextureEncoding;
use crate::viewport::renderer::TextureImage;
use crate::viewport::renderer::Viewport3D;
use crate::viewport::tangents::vertex_tangents;
use crate::viewport::types::MeshId;
use crate::viewport::types::Vertex;

use crate::camo::RgbaTexture;
use crate::load_ship::LoadedShipArmor;
use crate::load_ship::transform_normal;
use crate::surface_maps::SurfaceTextures;

/// Hull-mesh brightness boost for a mesh with no texture; the lighting
/// shader multiplies this base by `(flat + key*halfLambert)`. Matches the
/// egui original's `hull_brightness` constant (`tab.rs:1374`).
const HULL_BRIGHTNESS: f32 = 2.0;

/// Fallback per-texture brightness when a hull mesh has no texture data (or a
/// texture with no sampled pixels). Matches the egui original's
/// `unwrap_or(3.5)` (`tab.rs:1395`).
const FALLBACK_TEX_BRIGHTNESS: f32 = 3.5;

/// Target P95 luma [`tex_brightness`] scales toward. Matches the egui
/// original's `TARGET_HI` constant (`tab.rs:1392`).
const TARGET_HI: f32 = 0.85;

/// Sample every 37th RGBA pixel when computing a texture's P95 luma -- cheap
/// enough to run per-upload, dense enough for a stable percentile. Matches
/// the egui original's `step_by(37)` (`tab.rs:1384`).
const LUMA_SAMPLE_STRIDE: usize = 37;

/// Extracts the file stem (no directory, no `.mfm` extension) from an mfm
/// path, for matching a hull mesh's texture against `active_camo_textures`'s
/// keys. Ports `armor_viewer::common::mfm_stem` verbatim.
pub fn mfm_stem(mfm_path: &str) -> &str {
    let base = mfm_path.rsplit(['/', '\\']).next().unwrap_or(mfm_path);
    base.strip_suffix(".mfm").unwrap_or(base)
}

/// Computes a texture's brightness boost from the 95th-percentile luma of a
/// sampled subset of its pixels, scaling that percentile toward `TARGET_HI`
/// and clamping to `[1.0, 4.0]`. Pure function of `rgba` so it is testable
/// without a GPU. Matches the egui original's inline computation
/// (`tab.rs:1380-1395`) exactly, including the luma weights and stride: a
/// mean-based factor over-boosts a mostly-dark texture that has bright spots
/// (e.g. a camo with light stripes on a dark base); keying off the P95 bright
/// pixels caps the boost by the brightest regions directly, so they do not
/// saturate to white while dark textures still get boosted.
pub fn tex_brightness(rgba: &[u8]) -> f32 {
    let mut lumas: Vec<f32> = rgba
        .as_chunks::<4>()
        .0
        .iter()
        .step_by(LUMA_SAMPLE_STRIDE)
        .map(|px| (0.2126 * px[0] as f32 + 0.7152 * px[1] as f32 + 0.0722 * px[2] as f32) / 255.0)
        .collect();
    if lumas.is_empty() {
        return FALLBACK_TEX_BRIGHTNESS;
    }
    lumas.sort_by(|a, b| a.total_cmp(b));
    let p95 = lumas[(lumas.len() * 95 / 100).min(lumas.len() - 1)];
    (TARGET_HI / p95.max(0.05)).clamp(1.0, 4.0)
}

/// What a hull texture was made from.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum TextureKey {
    /// A material's own albedo, by `.mfm` path.
    Base(String),
    /// The active camo's texture, by `.mfm` stem.
    Camo(String),
}

/// An albedo and the `.mfm` path whose surface maps are drawn with it, if
/// that material has any.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct MaterialKey {
    albedo: TextureKey,
    surface: Option<String>,
}

/// The GPU textures one scene's hull meshes sample, made once per material
/// and shared by every mesh using it. Valid for one device, one scene and one
/// active camo: clear it when any of them changes.
#[derive(Default)]
pub struct HullTextureCache {
    albedos: HashMap<TextureKey, UploadedAlbedo>,
    /// By `.mfm` path.
    surface_maps: HashMap<String, UploadedSurfaceMaps>,
    materials: HashMap<MaterialKey, CachedMaterial>,
}

impl HullTextureCache {
    pub fn clear(&mut self) {
        self.albedos.clear();
        self.surface_maps.clear();
        self.materials.clear();
    }

    /// Drops the active camo's textures, keeping the materials' own.
    pub fn forget_camo(&mut self) {
        self.albedos.retain(|key, _| matches!(key, TextureKey::Base(_)));
        self.materials.retain(|key, _| matches!(key.albedo, TextureKey::Base(_)));
    }

    fn upload_albedo(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pipeline: &GpuPipeline,
        key: &TextureKey,
        texture: &RgbaTexture,
    ) {
        if self.albedos.contains_key(key) {
            return;
        }

        let view = upload(device, queue, pipeline, texture, TextureEncoding::Srgb);
        self.albedos.insert(key.clone(), UploadedAlbedo { view, brightness: tex_brightness(&texture.pixels) });
    }

    fn upload_surface_maps(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pipeline: &GpuPipeline,
        mfm_path: &str,
        textures: &SurfaceTextures,
    ) {
        if self.surface_maps.contains_key(mfm_path) {
            return;
        }

        let upload = |texture: &RgbaTexture| upload(device, queue, pipeline, texture, TextureEncoding::Linear);
        let uploaded = UploadedSurfaceMaps {
            normal: textures.normal.as_ref().map(upload),
            metal_gloss: textures.metal_gloss.as_ref().map(upload),
        };
        self.surface_maps.insert(mfm_path.to_string(), uploaded);
    }
}

/// One uploaded albedo and the brightness boost its pixels call for.
struct UploadedAlbedo {
    view: wgpu::TextureView,
    brightness: f32,
}

/// One material's uploaded surface maps.
struct UploadedSurfaceMaps {
    normal: Option<wgpu::TextureView>,
    metal_gloss: Option<wgpu::TextureView>,
}

/// One material's bind group and what its meshes need to know of it.
#[derive(Clone)]
struct CachedMaterial {
    bind_group: wgpu::BindGroup,
    brightness: f32,
    alpha: AlphaMode,
    normal_mapped: bool,
}

fn upload(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pipeline: &GpuPipeline,
    texture: &RgbaTexture,
    encoding: TextureEncoding,
) -> wgpu::TextureView {
    let mips = texture.take_mips();
    let image = TextureImage { pixels: &texture.pixels, width: texture.width, height: texture.height, mips: &mips };

    pipeline.upload_texture(device, queue, image, encoding)
}

/// Re-uploads `armor`'s hull meshes: removes every previously-uploaded hull
/// mesh (`hull_mesh_ids`, drained and replaced in place) and re-adds only the
/// ones `hull_visibility` marks visible (absent/false = hidden, matching the
/// egui app's own default). `hull_opaque` selects the alpha/layer: opaque
/// hulls draw depth-written on [`LAYER_DEFAULT`] under the armor, which
/// tints them; translucent ones draw on [`LAYER_HULL`] (no depth write, over
/// the armor).
/// `active_camo_textures`/`active_camo_uvs` are the host's decoded
/// active-camo state (see the module doc); pass empty maps for base albedo
/// only. A textured mesh whose material has surface maps draws them under
/// whichever albedo it shows.
///
/// Shares `viewport`'s mesh pool with the armor upload (`upload.rs`) but
/// never calls [`Viewport3D::clear`] itself, so an armor-only change never
/// touches the hull and vice versa -- except that the ARMOR path's own
/// `clear()` does wipe every mesh including the hull's, which
/// `viewport_view.rs::reupload_current_armor` resolves by calling this
/// function again right after, using the (unaffected) `hull_visibility`/
/// `hull_opaque` state.
#[allow(clippy::too_many_arguments)]
pub fn upload_hull_meshes(
    viewport: &mut Viewport3D,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pipeline: &GpuPipeline,
    armor: &LoadedShipArmor,
    hull_mesh_ids: &mut Vec<MeshId>,
    hull_visibility: &HashMap<String, bool>,
    hull_opaque: bool,
    active_camo_textures: &crate::camo::TexturesByPart,
    active_camo_uvs: &HashMap<String, UvTransform>,
    texture_cache: &mut HullTextureCache,
) {
    for mid in hull_mesh_ids.drain(..) {
        viewport.remove_mesh(mid);
    }

    let hull_alpha: f32 = if hull_opaque { 1.0 } else { 0.7 };
    let hull_layer = if hull_opaque { LAYER_DEFAULT } else { LAYER_HULL };
    let surface_maps = armor.surface_maps();

    for mesh in &armor.hull_meshes {
        let visible = hull_visibility.get(&mesh.name).copied().unwrap_or(false);
        if !visible {
            continue;
        }

        let has_uvs = mesh.uvs.len() == mesh.positions.len();
        let stem = mesh.mfm_path.as_deref().map(mfm_stem);
        // Active camo texture for this stem takes precedence; otherwise the base albedo.
        let camo_texture = stem.and_then(|s| active_camo_textures.get(s).map(|texture| (s, texture)));
        // A camo is drawn opaque over the hull; a material missing from
        // `hull_alpha` draws opaque, as that map lists only the others.
        let texture = match camo_texture {
            Some((stem, texture)) => Some((TextureKey::Camo(stem.to_string()), texture, AlphaMode::Opaque)),
            None => mesh.mfm_path.as_ref().and_then(|path| {
                let alpha = armor.hull_alpha.get(path).copied().unwrap_or(AlphaMode::Opaque);
                armor.hull_textures.get(path).map(|texture| (TextureKey::Base(path.clone()), texture, alpha))
            }),
        }
        .filter(|_| has_uvs);
        // Only apply a tiled UV transform when this stem actually has an
        // active camo texture; otherwise the mesh falls back to base albedo
        // with its own UVs.
        let albedo_uv = camo_texture
            .and_then(|(s, _)| active_camo_uvs.get(s))
            .map_or(AlbedoUv::IDENTITY, |t| AlbedoUv { scale: t.scale, offset: t.offset });
        let surface = mesh.mfm_path.as_ref().and_then(|path| surface_maps?.get(path).map(|maps| (path, maps)));
        let bound = texture.map(|(key, texture, alpha)| {
            let material_key = MaterialKey { albedo: key.clone(), surface: surface.map(|(path, _)| path.clone()) };
            if let Some(cached) = texture_cache.materials.get(&material_key) {
                return cached.clone();
            }

            texture_cache.upload_albedo(device, queue, pipeline, &key, texture);
            if let Some((path, maps)) = surface {
                texture_cache.upload_surface_maps(device, queue, pipeline, path, maps);
            }
            let albedo = &texture_cache.albedos[&key];
            let maps = surface.and_then(|(path, _)| texture_cache.surface_maps.get(path));
            let binding = MaterialBinding {
                albedo: &albedo.view,
                normal: maps.and_then(|maps| maps.normal.as_ref()),
                metal_gloss: maps.and_then(|maps| maps.metal_gloss.as_ref()),
                alpha_cutoff: alpha.cutoff(),
                albedo_uv,
            };
            let cached = CachedMaterial {
                normal_mapped: binding.normal.is_some(),
                brightness: albedo.brightness,
                bind_group: pipeline.create_material_bind_group(device, binding),
                alpha,
            };
            texture_cache.materials.insert(material_key, cached.clone());

            cached
        });

        let color = match &bound {
            Some(cached) => [cached.brightness, cached.brightness, cached.brightness, hull_alpha],
            None => [0.6 * HULL_BRIGHTNESS, 0.6 * HULL_BRIGHTNESS, 0.65 * HULL_BRIGHTNESS, hull_alpha],
        };
        // Blended texels are drawn without depth writes, behind which the
        // opaque hull still shows, like the see-through hull.
        let blended = bound.as_ref().is_some_and(|cached| cached.alpha == AlphaMode::Blend);
        let layer = if blended { LAYER_HULL } else { hull_layer };
        let vertices = hull_vertices(mesh, color, bound.as_ref().is_some_and(|cached| cached.normal_mapped));

        if mesh.indices.is_empty() {
            continue;
        }
        let mid = match bound {
            Some(cached) => {
                viewport.add_textured_non_pickable_mesh(device, &vertices, &mesh.indices, layer, cached.bind_group)
            }
            None => viewport.add_non_pickable_mesh(device, &vertices, &mesh.indices, layer),
        };
        viewport.set_lit(mid, true);
        hull_mesh_ids.push(mid);
    }

    viewport.mark_dirty();
}

/// `mesh`'s vertices placed by its transform and colored `color`. Tangents
/// are derived only for a normal-mapped material, the one reader of them.
fn hull_vertices(mesh: &InteractiveHullMesh, color: [f32; 4], with_tangents: bool) -> Vec<Vertex> {
    let has_uvs = mesh.uvs.len() == mesh.positions.len();
    let mut vertices: Vec<Vertex> = mesh
        .positions
        .iter()
        .enumerate()
        .map(|(i, &position)| {
            let normal = mesh.normals.get(i).copied().unwrap_or([0.0, 1.0, 0.0]);
            let (position, normal) = match &mesh.transform {
                Some(t) => (transform_point(t, position), transform_normal(t, normal)),
                None => (position, normal),
            };
            let uv = if has_uvs { mesh.uvs[i] } else { [0.0, 0.0] };

            Vertex { position, normal, color, uv, tangent: [0.0; 4] }
        })
        .collect();

    if with_tangents && has_uvs {
        let positions: Vec<[f32; 3]> = vertices.iter().map(|vertex| vertex.position).collect();
        let normals: Vec<[f32; 3]> = vertices.iter().map(|vertex| vertex.normal).collect();
        let tangents = vertex_tangents(&positions, &normals, &mesh.uvs, &mesh.indices);
        for (vertex, tangent) in vertices.iter_mut().zip(tangents) {
            vertex.tangent = tangent;
        }
    }

    vertices
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid_rgba(pixel: [u8; 4], count: usize) -> Vec<u8> {
        pixel.iter().copied().cycle().take(count * 4).collect()
    }

    /// A quad in the XY plane facing +Z with an upright texture, moved one
    /// unit along X by its transform.
    fn moved_quad() -> InteractiveHullMesh {
        InteractiveHullMesh {
            name: "Hull".to_string(),
            positions: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0]],
            normals: vec![[0.0, 0.0, 1.0]; 4],
            uvs: vec![[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]],
            indices: vec![0, 1, 2, 0, 2, 3],
            mfm_path: Some("content/Hull.mfm".to_string()),
            mfm_path_id: 0,
            transform: Some([1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 1.0]),
            group: wowsunpack::export::part_group::PartGroup::Hull,
            part: None,
        }
    }

    #[test]
    fn a_material_without_surface_maps_uploads_its_mesh_as_it_is_stored() {
        let mesh = moved_quad();
        let color = [2.0, 2.0, 2.0, 1.0];

        let vertices = hull_vertices(&mesh, color, false);

        assert_eq!(vertices.len(), 4);
        for (vertex, (position, uv)) in vertices.iter().zip(mesh.positions.iter().zip(&mesh.uvs)) {
            assert_eq!(vertex.position, [position[0] + 1.0, position[1], position[2]]);
            assert_eq!(vertex.normal, [0.0, 0.0, 1.0]);
            assert_eq!(vertex.color, color);
            assert_eq!(vertex.uv, *uv, "the albedo transform is applied when sampling, not here");
            assert_eq!(vertex.tangent, [0.0; 4]);
        }
    }

    #[test]
    fn a_normal_mapped_material_gets_tangents_from_the_placed_mesh() {
        let vertices = hull_vertices(&moved_quad(), [1.0; 4], true);

        assert!(vertices.iter().all(|vertex| vertex.tangent == [1.0, 0.0, 0.0, 1.0]), "{vertices:?}");
    }

    #[test]
    fn mfm_stem_strips_directory_and_extension() {
        assert_eq!(mfm_stem("content/gameplay/hull/Hull_A.mfm"), "Hull_A");
        assert_eq!(mfm_stem("content\\gameplay\\hull\\Hull_B.mfm"), "Hull_B");
        assert_eq!(mfm_stem("NoExtension"), "NoExtension");
    }

    #[test]
    fn tex_brightness_falls_back_when_the_texture_is_empty() {
        assert_eq!(tex_brightness(&[]), FALLBACK_TEX_BRIGHTNESS);
    }

    #[test]
    fn tex_brightness_clamps_to_one_for_an_already_bright_texture() {
        // Every sampled pixel is near-white (luma ~1.0): TARGET_HI / 1.0 < 1.0,
        // so the clamp floor of 1.0 applies (never darkens a texture).
        let rgba = solid_rgba([255, 255, 255, 255], LUMA_SAMPLE_STRIDE * 4);
        assert_eq!(tex_brightness(&rgba), 1.0);
    }

    #[test]
    fn tex_brightness_clamps_to_four_for_a_very_dark_texture() {
        // Every sampled pixel is near-black (luma ~0.0): p95.max(0.05) floors
        // the denominator at 0.05, giving 0.85 / 0.05 = 17.0, clamped to 4.0.
        let rgba = solid_rgba([0, 0, 0, 255], LUMA_SAMPLE_STRIDE * 4);
        assert_eq!(tex_brightness(&rgba), 4.0);
    }

    #[test]
    fn tex_brightness_targets_the_p95_luma_for_a_mid_gray_texture() {
        // A uniform luma L means p95 == L (every sample is the same), so the
        // boost is exactly TARGET_HI / L for an L that keeps the ratio inside
        // [1.0, 4.0] -- pick L = 0.5 so 0.85 / 0.5 = 1.7, comfortably clamped.
        let rgba = solid_rgba([128, 128, 128, 255], LUMA_SAMPLE_STRIDE * 4);
        let luma: f32 = (0.2126 * 128.0 + 0.7152 * 128.0 + 0.0722 * 128.0) / 255.0;
        let expected = (TARGET_HI / luma.max(0.05)).clamp(1.0, 4.0);
        assert!((tex_brightness(&rgba) - expected).abs() < 1e-4);
    }

    /// Renders real ships with their armor drawn over the full hull from
    /// several angles, with the hull translucent and opaque, and writes each
    /// frame as a PNG to review the two surfaces where they coincide. Needs a
    /// game install and a real GPU adapter. Run with:
    ///
    /// ```text
    /// WOWS_ARMOR_VIEWER_LOAD_TEST_DIR="E:\WoWs\World_of_Warships" \
    /// WOWS_ARMOR_VIEWER_SHOT_DIR=/tmp/armor-shots \
    /// cargo test -p wows-toolkit-armor -- --ignored --nocapture armor_over_the_hull_renders_for_review
    /// ```
    #[test]
    #[ignore = "needs a local game install and a real GPU adapter; see the doc comment for the run command"]
    fn armor_over_the_hull_renders_for_review() {
        use std::sync::Arc;

        use crate::upload::DisplaySettings;
        use crate::upload::upload_armor_to_viewport;
        use crate::viewport::headless::HeadlessDevice;
        use crate::viewport::headless::SRGB_TARGET;
        use crate::viewport::types::TargetSize;
        use crate::visibility::VisibilityFilter;

        let wows_dir = std::env::var("WOWS_ARMOR_VIEWER_LOAD_TEST_DIR")
            .expect("set WOWS_ARMOR_VIEWER_LOAD_TEST_DIR to a WoWs install directory");
        let out_dir = std::env::var("WOWS_ARMOR_VIEWER_SHOT_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| std::env::temp_dir().join("wows-toolkit-armor-shots"));
        std::fs::create_dir_all(&out_dir).expect("the output directory is created");
        let wows_dir = std::path::PathBuf::from(wows_dir);
        let available =
            wowsunpack::game_data::list_available_builds(&wows_dir).expect("failed to list installed builds");
        let build = *available.last().expect("expected at least one installed build");
        let vfs = wowsunpack::game_data::build_game_vfs_for_build(
            &wows_dir,
            build,
            wowsunpack::game_data::AssetsBin::Overlay,
        )
        .expect("failed to build the VFS");
        let metadata = Arc::new(
            wowsunpack::game_params::provider::GameMetadataProvider::from_vfs(&vfs)
                .expect("failed to build GameMetadataProvider from the VFS"),
        );
        let ship_assets = wowsunpack::export::ship::ShipAssets::from_vfs_with_metadata(&vfs, Arc::clone(&metadata))
            .expect("failed to load ShipAssets from the VFS");
        let headless = HeadlessDevice::new(SRGB_TARGET).expect("a wgpu adapter is available");
        let size = TargetSize::new(1280, 720).expect("a nonzero size");
        let empty_part = HashMap::new();
        let empty_plate = HashMap::new();
        let visibility = VisibilityFilter { part: &empty_part, plate: &empty_plate, show_hidden_only: false };
        // Azimuth and elevation in radians, and distance as a share of the framed distance.
        let views: [(&str, f32, f32, f32); 4] =
            [("side", 1.25, 0.2, 0.7), ("bow", 0.5, 0.35, 0.8), ("above", 1.9, 0.9, 0.8), ("close", 1.4, 0.1, 0.3)];

        for (name, param_index) in [("Yamato", "PJSB018"), ("Des_Moines", "PASC020")] {
            let armor = crate::load_ship::load_ship_armor_by_param(
                &ship_assets,
                param_index,
                name,
                &crate::scene_load::LoadCancel::new(),
            )
            .unwrap_or_else(|e| panic!("failed to load {name}: {e}"))
            .scene;
            println!("{name}: bounds {:?}", armor.bounds);
            let hull_visibility: HashMap<String, bool> =
                armor.hull_meshes.iter().map(|mesh| (mesh.name.clone(), true)).collect();

            for hull_opaque in [false, true] {
                let mut viewport = Viewport3D::new();
                let display = DisplaySettings { hull_opaque, ..DisplaySettings::default() };
                upload_armor_to_viewport(&mut viewport, &headless.device, &armor, visibility, display);
                let mut hull_mesh_ids = Vec::new();
                let mut texture_cache = HullTextureCache::default();
                upload_hull_meshes(
                    &mut viewport,
                    &headless.device,
                    &headless.queue,
                    &headless.pipeline,
                    &armor,
                    &mut hull_mesh_ids,
                    &hull_visibility,
                    hull_opaque,
                    &HashMap::new(),
                    &HashMap::new(),
                    &mut texture_cache,
                );
                let framed = viewport.camera.clone();

                for (view, azimuth, elevation, distance) in views {
                    viewport.camera = framed.clone();
                    viewport.camera.azimuth = azimuth;
                    viewport.camera.elevation = elevation;
                    viewport.camera.distance = framed.distance * distance;
                    let rgba = headless.render_rgba(&viewport, size);
                    let hull = if hull_opaque { "opaque" } else { "translucent" };
                    let path = out_dir.join(format!("{name}-{hull}-{view}.png"));
                    image::save_buffer(&path, &rgba, size.width(), size.height(), image::ExtendedColorType::Rgba8)
                        .expect("the frame is written");
                    println!("{}", path.display());
                }
            }
        }
    }
}
