//! Export ship visual + geometry to glTF/GLB format.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::HashSet;
use std::io::Read;
use std::io::Write;

use gltf_json as json;
use json::validation::Checked::Valid;
use json::validation::USize64;
use rootcause::Report;
use thiserror::Error;

use crate::game_params::types::ArmorMap;
use crate::models::assets_bin::PrototypeDatabase;
use crate::models::geometry::MergedGeometry;
use crate::models::merged_models::MergedModels;
use crate::models::merged_models::SpaceInstances;
use crate::models::speedtree::SpeedTreeMesh;
use crate::models::terrain::Terrain;
use crate::models::vertex_format::AttributeSemantic;
use crate::models::vertex_format::VertexFormat;
use crate::models::vertex_format::{
    self,
};
use crate::models::visual::VisualPrototype;

use super::texture;

#[derive(Debug, Error)]
pub enum ExportError {
    #[error("no LOD {0} in visual (max LOD: {1})")]
    LodOutOfRange(usize, usize),
    #[error("no .visual files found in directory: {0}")]
    NoVisualFiles(String),
    #[error("render set name 0x{0:08X} not found among render sets")]
    RenderSetNotFound(u32),
    #[error("vertices mapping id 0x{id:08X} not found in geometry")]
    VerticesMappingNotFound { id: u32 },
    #[error("indices mapping id 0x{id:08X} not found in geometry")]
    IndicesMappingNotFound { id: u32 },
    #[error("buffer index {index} out of range (count: {count})")]
    BufferIndexOutOfRange { index: usize, count: usize },
    #[error("vertex decode error: {0}")]
    VertexDecode(String),
    #[error("index decode error: {0}")]
    IndexDecode(String),
    #[error("vertex format stride mismatch: format says {format_stride}, geometry says {geo_stride}")]
    StrideMismatch { format_stride: usize, geo_stride: usize },
    #[error("glTF serialization error: {0}")]
    Serialize(String),
    #[error("I/O error: {0}")]
    Io(String),
}

/// Decoded primitive data ready for glTF export.
struct DecodedPrimitive {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    indices: Vec<u32>,
    material_name: String,
    /// MFM stem for texture lookup (e.g. "JSB039_Yamato_1945_Hull").
    mfm_stem: Option<String>,
    /// Full VFS path to the .mfm file (e.g. "content/location/.../textures/LBC001.mfm").
    mfm_full_path: Option<String>,
    /// Raw selfId for the .mfm file in assets.bin (used for TILEDLAND MFM parsing).
    mfm_path_id: u64,
}

/// Export a visual + geometry pair to a GLB binary and write it.
///
/// `texture_set` contains base albedo PNGs and optional camouflage variant PNGs.
/// Primitives whose MFM stem matches a key will have the texture applied as
/// `baseColorTexture` on their material. Camo variants are exposed via
/// `KHR_materials_variants` so users can switch in Blender.
pub fn export_glb(
    visual: &VisualPrototype,
    geometry: &MergedGeometry,
    db: &PrototypeDatabase<'_>,
    lod: usize,
    texture_set: &TextureSet,
    damaged: bool,
    writer: &mut impl Write,
) -> Result<(), Report<ExportError>> {
    if visual.lods.is_empty() {
        return Err(Report::new(ExportError::LodOutOfRange(lod, 0)));
    }
    if lod >= visual.lods.len() {
        return Err(Report::new(ExportError::LodOutOfRange(lod, visual.lods.len() - 1)));
    }

    let lod_entry = &visual.lods[lod];

    // Collect render sets for this LOD by matching LOD render_set_names to RS name_ids.
    let self_id_index = db.build_self_id_index();
    let primitives = collect_primitives(visual, geometry, Some(db), Some(&self_id_index), lod_entry, damaged, None)?;

    if primitives.is_empty() {
        eprintln!("Warning: no primitives found for LOD {lod}");
    }

    // Build glTF document.
    let mut root = json::Root {
        asset: json::Asset {
            version: "2.0".to_string(),
            generator: Some("wowsunpack".to_string()),
            ..Default::default()
        },
        ..Default::default()
    };

    // Accumulate all binary data into a single buffer.
    let mut bin_data: Vec<u8> = Vec::new();
    let mut gltf_primitives = Vec::new();
    let mut mat_cache = MaterialCache::new();

    for prim in &primitives {
        let gltf_prim = add_primitive_to_root(&mut root, &mut bin_data, prim, texture_set, &mut mat_cache)?;
        gltf_primitives.push(gltf_prim);
    }

    // Pad binary data to 4-byte alignment.
    while !bin_data.len().is_multiple_of(4) {
        bin_data.push(0);
    }

    // Set the buffer byte_length now that we know the total size.
    if !bin_data.is_empty() {
        let buffer = root.push(json::Buffer {
            byte_length: USize64::from(bin_data.len()),
            uri: None,
            name: None,
            extensions: Default::default(),
            extras: Default::default(),
        });
        // Update all buffer views to reference this buffer.
        for bv in root.buffer_views.iter_mut() {
            bv.buffer = buffer;
        }
    }

    // Create mesh with all primitives.
    let mesh = root.push(json::Mesh {
        primitives: gltf_primitives,
        weights: None,
        name: None,
        extensions: Default::default(),
        extras: Default::default(),
    });

    // Create a simple node hierarchy.
    // For now, use a single root node with the mesh attached.
    let root_node = root.push(json::Node { mesh: Some(mesh), ..Default::default() });

    let scene = root.push(json::Scene {
        nodes: vec![root_node],
        name: None,
        extensions: Default::default(),
        extras: Default::default(),
    });
    root.scene = Some(scene);

    // Add KHR_materials_variants root extension if we have camo schemes.
    add_variants_extension(&mut root, texture_set);

    // Serialize and write GLB.
    let json_string =
        json::serialize::to_string(&root).map_err(|e| Report::new(ExportError::Serialize(e.to_string())))?;

    let glb = gltf::binary::Glb {
        header: gltf::binary::Header {
            magic: *b"glTF",
            version: 2,
            length: 0, // to_writer computes this
        },
        json: Cow::Owned(json_string.into_bytes()),
        bin: if bin_data.is_empty() { None } else { Some(Cow::Owned(bin_data)) },
    };

    glb.to_writer(writer).map_err(|e| Report::new(ExportError::Io(e.to_string())))?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Map scene: format-agnostic intermediate representation
// ---------------------------------------------------------------------------

/// World-space bounds for a map, derived from space.settings chunk coordinates.
#[derive(Debug, Clone)]
pub struct SpaceBounds {
    /// Minimum world X (chunk min × 100).
    pub min_x: f32,
    /// Maximum world X ((chunk max + 1) × 100).
    pub max_x: f32,
    /// Minimum world Z (chunk min × 100, row axis).
    pub min_z: f32,
    /// Maximum world Z ((chunk max + 1) × 100).
    pub max_z: f32,
}

/// Configuration for terrain mesh generation.
pub struct TerrainConfig<'a> {
    pub terrain: &'a Terrain,
    pub bounds: &'a SpaceBounds,
    /// Decimation step: 1 = full res, 4 = default (~858K tris), 8 = coarse.
    pub step: u32,
    /// Sea level height. Terrain vertices below this are clamped; fully
    /// submerged cells are culled to avoid ugly seabed through translucent water.
    pub sea_level: f32,
    /// VFS path to the lightmap shadow DDS, if available (e.g. from space.ubersettings).
    pub lightmap_path: Option<String>,
}

/// Configuration for water plane generation.
pub struct WaterConfig<'a> {
    pub bounds: &'a SpaceBounds,
    pub sea_level: f32,
}

/// All optional map environment layers.
pub struct MapEnvironment<'a> {
    pub terrain: Option<TerrainConfig<'a>>,
    pub water: Option<WaterConfig<'a>>,
}

/// A decoded mesh primitive (one render set, terrain, or water).
pub struct MapMesh {
    /// Human-readable name (e.g. render set name or "Terrain").
    pub name: String,
    /// Vertex positions (right-handed: Z negated).
    pub positions: Vec<[f32; 3]>,
    /// Vertex normals (same length as positions).
    pub normals: Vec<[f32; 3]>,
    /// Vertex UVs (same length as positions, or empty).
    pub uvs: Vec<[f32; 2]>,
    /// Triangle indices into the vertex arrays.
    pub indices: Vec<u32>,
    /// Index into [`MapScene::textures`] for the albedo texture, if any.
    pub albedo_texture: Option<usize>,
    /// Base color factor (used when no texture). RGBA linear.
    pub base_color: [f32; 4],
    /// Alpha blending mode: false = opaque, true = blend.
    pub alpha_blend: bool,
    /// Alpha cutoff for mask mode (e.g. `Some(0.5)` for leaf transparency).
    pub alpha_cutoff: Option<f32>,
}

/// A positioned model instance in the map.
pub struct MapModelInstance {
    /// Range of indices into [`MapScene::model_meshes`] for this model's primitives.
    pub mesh_range: std::ops::Range<usize>,
    /// Column-major 4×4 world transform (right-handed, Z negated).
    pub transform: [f32; 16],
}

/// Complete decoded map scene, format-agnostic.
///
/// All geometry is decoded and textures are loaded. This struct can be consumed
/// by a GLB exporter, an egui renderer, or any other visualization backend.
pub struct MapScene {
    /// Unique mesh primitives for instanced models (render sets, grouped per model).
    pub model_meshes: Vec<MapMesh>,
    /// Positioned model instances referencing `model_meshes` by range.
    pub model_instances: Vec<MapModelInstance>,
    /// Shared albedo textures (PNG bytes). Meshes reference these by index.
    pub textures: Vec<Vec<u8>>,
    /// Terrain mesh, if generated.
    pub terrain: Option<MapMesh>,
    /// Water plane mesh, if generated.
    pub water: Option<MapMesh>,
    /// World-space bounds of the map.
    pub bounds: SpaceBounds,
    /// GPU-instanced vegetation: `(mesh_idx, positions)` per species.
    /// Exported as `EXT_mesh_gpu_instancing` nodes (one node per species).
    pub vegetation_instances: Vec<(usize, Vec<[f32; 3]>)>,
}

/// Cache key for deduplicating map materials by visual parameters.
///
/// Float fields are stored as `f32::to_bits()` so the key is `Hash + Eq`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct MapMaterialKey {
    albedo_texture: Option<usize>,
    base_color_bits: [u32; 4],
    alpha_blend: bool,
    alpha_cutoff_bits: Option<u32>,
}

/// Material cache mapping unique material parameters to their glTF index.
type MapMaterialCache = HashMap<MapMaterialKey, json::Index<json::Material>>;

/// A single SpeedTree species with its mesh and optional albedo texture.
pub struct VegetationSpecies {
    pub mesh: SpeedTreeMesh,
    pub albedo_png: Option<Vec<u8>>,
}

/// Vegetation data: species meshes + positioned instances.
pub struct VegetationData {
    pub species: Vec<VegetationSpecies>,
    /// `(species_index, world_position [x, y, z])` per instance.
    pub instances: Vec<(usize, [f32; 3])>,
}

/// Build a complete map scene from parsed data.
///
/// Decodes all model geometry, loads textures on demand, generates terrain mesh
/// and water plane as configured. The returned [`MapScene`] is format-agnostic
/// and can be serialized to GLB via [`export_map_scene_glb`] or consumed
/// directly by a renderer.
/// Parameters for [`build_map_scene`].
pub struct BuildMapSceneParams<'a> {
    pub merged: &'a MergedModels,
    pub geometry: &'a MergedGeometry<'a>,
    pub space: Option<&'a SpaceInstances>,
    pub db: Option<&'a PrototypeDatabase<'a>>,
    pub lod: usize,
    pub vfs: Option<&'a vfs::VfsPath>,
    pub env: &'a MapEnvironment<'a>,
    pub bounds: SpaceBounds,
    pub max_texture_size: Option<u32>,
    pub vegetation: Option<&'a VegetationData>,
    pub vegetation_density: f32,
}

pub fn build_map_scene(params: &BuildMapSceneParams<'_>) -> Result<MapScene, Report<ExportError>> {
    let BuildMapSceneParams {
        merged,
        geometry,
        space,
        db,
        lod,
        vfs,
        env,
        ref bounds,
        max_texture_size,
        vegetation,
        vegetation_density,
    } = *params;
    let self_id_index = db.map(|db| db.build_self_id_index());

    // Shared texture storage: each unique texture is stored once.
    let mut textures: Vec<Vec<u8>> = Vec::new();
    // Cache: MFM full path → Option<texture index>
    let mut texture_cache: HashMap<String, Option<usize>> = HashMap::new();

    // Build path_id → model index map for instance lookups.
    let path_to_model: HashMap<u64, usize> = merged.models.iter().enumerate().map(|(i, r)| (r.path_id, i)).collect();

    // Decode model meshes: one set of MapMesh entries per model prototype.
    // model_mesh_ranges[i] = range in model_meshes for model index i.
    let mut model_meshes: Vec<MapMesh> = Vec::new();
    let mut model_mesh_ranges: Vec<std::ops::Range<usize>> = Vec::new();

    for (model_idx, record) in merged.models.iter().enumerate() {
        let vp = &record.visual_proto;
        let range_start = model_meshes.len();

        if vp.lods.is_empty() || lod >= vp.lods.len() {
            model_mesh_ranges.push(range_start..range_start);
            continue;
        }

        let lod_entry = &vp.lods[lod];
        let primitives = match collect_primitives(vp, geometry, db, self_id_index.as_ref(), lod_entry, false, None) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("Warning: model[{model_idx}]: {e}");
                model_mesh_ranges.push(range_start..range_start);
                continue;
            }
        };

        for prim in primitives {
            // Load texture on demand via full MFM path (deduplicated).
            // Falls back to TILEDLAND baking for terrain materials.
            let albedo_texture = if let Some(vfs) = vfs
                && let Some(mfm_path) = &prim.mfm_full_path
            {
                *texture_cache.entry(mfm_path.clone()).or_insert_with(|| {
                    texture::load_or_bake_albedo(
                        vfs,
                        mfm_path,
                        prim.mfm_path_id,
                        db,
                        self_id_index.as_ref(),
                        max_texture_size,
                    )
                    .map(|png_bytes| {
                        let idx = textures.len();
                        textures.push(png_bytes);
                        idx
                    })
                })
            } else {
                None
            };

            model_meshes.push(MapMesh {
                name: prim.material_name,
                positions: prim.positions,
                normals: prim.normals,
                uvs: prim.uvs,
                indices: prim.indices,
                albedo_texture,
                base_color: [1.0, 1.0, 1.0, 1.0],
                alpha_blend: false,
                alpha_cutoff: None,
            });
        }

        model_mesh_ranges.push(range_start..model_meshes.len());
    }

    // Build model instances from space.bin transforms.
    let mut model_instances: Vec<MapModelInstance> = Vec::new();
    let mut vegetation_instances: Vec<(usize, Vec<[f32; 3]>)> = Vec::new();
    if let Some(space) = space {
        for inst in &space.instances {
            let Some(&model_idx) = path_to_model.get(&inst.path_id) else {
                continue;
            };
            let range = &model_mesh_ranges[model_idx];
            if range.is_empty() {
                continue;
            }

            model_instances.push(MapModelInstance { mesh_range: range.clone(), transform: inst.transform.0 });
        }
    } else {
        // No space.bin: place each model at origin.
        for (model_idx, range) in model_mesh_ranges.iter().enumerate() {
            if range.is_empty() {
                continue;
            }
            let _ = model_idx;
            model_instances.push(MapModelInstance {
                mesh_range: range.clone(),
                transform: [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0],
            });
        }
    }

    // Generate terrain mesh, optionally with lightmap texture.
    let terrain = env.terrain.as_ref().map(|cfg| {
        let mut mesh = generate_terrain_mesh(cfg);

        // Try to load the lightmap shadow DDS as terrain albedo.
        if let Some(vfs) = vfs
            && let Some(lm_path) = &cfg.lightmap_path
        {
            let dds_bytes: Option<Vec<u8>> = (|| {
                let mut buf = Vec::new();
                vfs.join(lm_path).ok()?.open_file().ok()?.read_to_end(&mut buf).ok()?;
                if buf.is_empty() { None } else { Some(buf) }
            })();
            match dds_bytes {
                Some(dds_bytes) => match texture::dds_to_png_resized(&dds_bytes, max_texture_size) {
                    Ok(mut png_bytes) => {
                        // Force alpha=255: the lightmap DDS stores shadow data in
                        // the alpha channel which causes terrain transparency in viewers.
                        texture::force_png_opaque(&mut png_bytes);
                        let idx = textures.len();
                        textures.push(png_bytes);
                        mesh.albedo_texture = Some(idx);
                        mesh.base_color = [1.0, 1.0, 1.0, 1.0];
                        eprintln!("  Terrain lightmap loaded: {lm_path}");
                    }
                    Err(e) => eprintln!("  Warning: failed to decode terrain lightmap: {e}"),
                },
                None => eprintln!("  Warning: terrain lightmap not found: {lm_path}"),
            }
        }

        mesh
    });

    // Generate water plane.
    let water = env.water.as_ref().map(generate_water_mesh);

    // Add vegetation meshes and instances.
    if let Some(veg) = vegetation {
        // Map species index → mesh range in model_meshes.
        let mut species_mesh_ranges: Vec<Option<usize>> = Vec::new();

        for (sp_idx, species) in veg.species.iter().enumerate() {
            if species.mesh.positions.is_empty() || species.mesh.indices.is_empty() {
                species_mesh_ranges.push(None);
                continue;
            }

            let albedo_texture = species.albedo_png.as_ref().map(|png| {
                let idx = textures.len();
                textures.push(png.clone());
                idx
            });

            let mesh_idx = model_meshes.len();
            model_meshes.push(MapMesh {
                name: format!("Vegetation_{sp_idx}"),
                positions: species.mesh.positions.clone(),
                normals: species.mesh.normals.clone(),
                uvs: species.mesh.uvs.clone(),
                indices: species.mesh.indices.clone(),
                albedo_texture,
                base_color: [1.0, 1.0, 1.0, 1.0],
                alpha_blend: false,
                alpha_cutoff: Some(0.5),
            });
            species_mesh_ranges.push(Some(mesh_idx));
        }

        // Group instances by species, with optional grid-based decimation.
        let num_species = veg.species.len();
        let mut per_species: Vec<Vec<[f32; 3]>> = vec![Vec::new(); num_species];
        let mut kept = 0usize;

        if vegetation_density > 0.0 {
            let inv_cell = 1.0 / vegetation_density;
            let mut occupied: HashSet<(usize, i32, i32)> = HashSet::new();
            for &(sp_idx, [x, y, z]) in &veg.instances {
                if species_mesh_ranges.get(sp_idx).and_then(|v| *v).is_none() {
                    continue;
                }
                let cx = (x * inv_cell).floor() as i32;
                let cz = (z * inv_cell).floor() as i32;
                if !occupied.insert((sp_idx, cx, cz)) {
                    continue;
                }
                per_species[sp_idx].push([x, y, -z]);
                kept += 1;
            }
        } else {
            for &(sp_idx, [x, y, z]) in &veg.instances {
                if species_mesh_ranges.get(sp_idx).and_then(|v| *v).is_none() {
                    continue;
                }
                per_species[sp_idx].push([x, y, -z]);
                kept += 1;
            }
        }

        // Collect into vegetation_instances (mesh_idx, positions) per species.
        for (sp_idx, positions) in per_species.into_iter().enumerate() {
            if positions.is_empty() {
                continue;
            }
            if let Some(Some(mesh_idx)) = species_mesh_ranges.get(sp_idx) {
                vegetation_instances.push((*mesh_idx, positions));
            }
        }

        eprintln!(
            "  Vegetation: {} species, {} instances (kept {kept}, cell {vegetation_density}m)",
            veg.species.len(),
            veg.instances.len(),
        );
    }

    let tex_tried = texture_cache.len();
    let tex_loaded = textures.len();
    eprintln!(
        "Map scene: {} model meshes, {} instances, {tex_loaded}/{tex_tried} textures loaded",
        model_meshes.len(),
        model_instances.len(),
    );
    if terrain.is_some() {
        eprintln!("  Terrain mesh generated");
    }
    if water.is_some() {
        eprintln!("  Water plane generated");
    }

    Ok(MapScene {
        model_meshes,
        model_instances,
        textures,
        terrain,
        water,
        bounds: bounds.clone(),
        vegetation_instances,
    })
}

/// Generate a terrain mesh from the heightmap.
fn generate_terrain_mesh(cfg: &TerrainConfig<'_>) -> MapMesh {
    let terrain = cfg.terrain;
    let bounds = cfg.bounds;
    let step = cfg.step.max(1);
    let sea = cfg.sea_level;

    let src_w = terrain.width as usize;
    let src_h = terrain.height as usize;

    // Output grid dimensions (decimated).
    let out_w = (src_w - 1) / step as usize + 1;
    let out_h = (src_h - 1) / step as usize + 1;

    let world_width = bounds.max_x - bounds.min_x;
    let world_depth = bounds.max_z - bounds.min_z;
    let cell_x = world_width / (src_w - 1) as f32;
    let cell_z = world_depth / (src_h - 1) as f32;

    // Helper: read height from source heightmap, clamped to sea level.
    let height_at = |sx: usize, sy: usize| -> f32 { terrain.heightmap[sy * src_w + sx].max(sea) };

    // First pass: determine which output grid vertices are above sea level
    // (i.e. NOT clamped flat at sea). We only emit triangles where at least
    // one vertex is above sea level, to cull fully-submerged flat seabed.
    let mut above_sea = vec![false; out_w * out_h];
    for gy in 0..out_h {
        let sy = (gy * step as usize).min(src_h - 1);
        for gx in 0..out_w {
            let sx = (gx * step as usize).min(src_w - 1);
            above_sea[gy * out_w + gx] = terrain.heightmap[sy * src_w + sx] > sea;
        }
    }

    let vert_count = out_w * out_h;
    let mut positions = Vec::with_capacity(vert_count);
    let mut normals = Vec::with_capacity(vert_count);
    let mut uvs = Vec::with_capacity(vert_count);

    for gy in 0..out_h {
        let sy = (gy * step as usize).min(src_h - 1);
        for gx in 0..out_w {
            let sx = (gx * step as usize).min(src_w - 1);

            let world_x = bounds.min_x + sx as f32 * cell_x;
            let world_z = bounds.min_z + sy as f32 * cell_z;
            let height = height_at(sx, sy);

            // Negate Z for right-handed coordinates.
            positions.push([world_x, height, -world_z]);

            // UV: normalized [0..1].
            let u = sx as f32 / (src_w - 1) as f32;
            let v = sy as f32 / (src_h - 1) as f32;
            uvs.push([u, v]);
        }
    }

    // Compute normals via central differences on the clamped heightmap.
    for gy in 0..out_h {
        let sy = (gy * step as usize).min(src_h - 1);
        for gx in 0..out_w {
            let sx = (gx * step as usize).min(src_w - 1);

            let sx_left = sx.saturating_sub(step as usize);
            let sx_right = (sx + step as usize).min(src_w - 1);
            let sy_up = sy.saturating_sub(step as usize);
            let sy_down = (sy + step as usize).min(src_h - 1);

            let h_left = height_at(sx_left, sy);
            let h_right = height_at(sx_right, sy);
            let h_up = height_at(sx, sy_up);
            let h_down = height_at(sx, sy_down);

            let dx = (sx_right - sx_left) as f32 * cell_x;
            let dz = (sy_down - sy_up) as f32 * cell_z;

            let dh_x = h_right - h_left;
            let dh_z = h_down - h_up;

            // In right-handed coords (Z negated on export):
            //   tangent_x = (dx, dh_x, 0)
            //   tangent_z = (0, dh_z, -dz)
            //   normal = tangent_x × tangent_z = (-dh_x*dz, dx*dz, dx*dh_z)
            let nx = -dh_x * dz;
            let ny = dx * dz;
            let nz = dx * dh_z;
            let len = (nx * nx + ny * ny + nz * nz).sqrt();
            if len > 1e-10 {
                normals.push([nx / len, ny / len, nz / len]);
            } else {
                normals.push([0.0, 1.0, 0.0]);
            }
        }
    }

    // Generate triangle indices, culling fully-submerged cells.
    let mut indices = Vec::new();

    for gy in 0..(out_h - 1) {
        for gx in 0..(out_w - 1) {
            let tl_idx = gy * out_w + gx;
            let tr_idx = tl_idx + 1;
            let bl_idx = (gy + 1) * out_w + gx;
            let br_idx = bl_idx + 1;

            // Skip cells where all four corners are at or below sea level.
            if !above_sea[tl_idx] && !above_sea[tr_idx] && !above_sea[bl_idx] && !above_sea[br_idx] {
                continue;
            }

            let tl = tl_idx as u32;
            let tr = tr_idx as u32;
            let bl = bl_idx as u32;
            let br = br_idx as u32;

            // Two triangles per cell. Winding for right-handed (CCW front).
            indices.push(tl);
            indices.push(bl);
            indices.push(tr);

            indices.push(tr);
            indices.push(bl);
            indices.push(br);
        }
    }

    eprintln!(
        "  Terrain: {}×{} grid (step {}), {} vertices, {} triangles (culled submerged)",
        out_w,
        out_h,
        step,
        positions.len(),
        indices.len() / 3,
    );

    MapMesh {
        name: "Terrain".to_string(),
        positions,
        normals,
        uvs,
        indices,
        albedo_texture: None,
        base_color: [0.3, 0.35, 0.25, 1.0],
        alpha_blend: false,
        alpha_cutoff: None,
    }
}

/// Generate a water plane quad.
fn generate_water_mesh(cfg: &WaterConfig<'_>) -> MapMesh {
    let bounds = cfg.bounds;
    let y = cfg.sea_level;

    // Four corners of the water plane, Z negated for right-handed.
    let positions = vec![
        [bounds.min_x, y, -bounds.min_z],
        [bounds.max_x, y, -bounds.min_z],
        [bounds.max_x, y, -bounds.max_z],
        [bounds.min_x, y, -bounds.max_z],
    ];
    let normals = vec![[0.0, 1.0, 0.0]; 4];
    let uvs = vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];

    // Two triangles (CCW winding for right-handed).
    let indices = vec![0, 3, 1, 1, 3, 2];

    MapMesh {
        name: "Water".to_string(),
        positions,
        normals,
        uvs,
        indices,
        albedo_texture: None,
        base_color: [0.1, 0.3, 0.5, 0.85],
        alpha_blend: true,
        alpha_cutoff: None,
    }
}

// ---------------------------------------------------------------------------
// GLB serialization for MapScene
// ---------------------------------------------------------------------------

/// Serialize a [`MapScene`] to GLB format.
pub fn export_map_scene_glb(scene: &MapScene, writer: &mut impl Write) -> Result<(), Report<ExportError>> {
    let mut root = json::Root {
        asset: json::Asset {
            version: "2.0".to_string(),
            generator: Some("wowsunpack".to_string()),
            ..Default::default()
        },
        ..Default::default()
    };

    let mut bin_data: Vec<u8> = Vec::new();

    // Embed shared textures once, cache glTF texture index per texture array index.
    let mut gltf_texture_cache: HashMap<usize, json::Index<json::Texture>> = HashMap::new();
    for (tex_idx, png_bytes) in scene.textures.iter().enumerate() {
        let byte_offset = bin_data.len();
        bin_data.extend_from_slice(png_bytes);
        pad_to_4(&mut bin_data);

        let bv = root.push(json::buffer::View {
            buffer: json::Index::new(0),
            byte_length: USize64::from(png_bytes.len()),
            byte_offset: Some(USize64::from(byte_offset)),
            byte_stride: None,
            target: None,
            name: None,
            extensions: Default::default(),
            extras: Default::default(),
        });
        let image = root.push(json::Image {
            buffer_view: Some(bv),
            mime_type: Some(json::image::MimeType("image/png".to_string())),
            uri: None,
            name: Some(format!("texture_{tex_idx}")),
            extensions: Default::default(),
            extras: Default::default(),
        });
        let tex = root.push(json::Texture {
            source: image,
            sampler: None,
            name: None,
            extensions: Default::default(),
            extras: Default::default(),
        });
        gltf_texture_cache.insert(tex_idx, tex);
    }

    // Material cache: (texture_index or None, base_color, alpha_blend) → glTF material.
    // This deduplicates materials that share the same texture + color + blend mode.
    let mut mat_cache: MapMaterialCache = HashMap::new();

    // glTF mesh cache: mesh index → glTF Mesh index.
    let mut gltf_mesh_cache: HashMap<usize, json::Index<json::Mesh>> = HashMap::new();

    let mut scene_nodes: Vec<json::Index<json::Node>> = Vec::new();

    // Helper closure args collected into a struct to avoid borrowing issues.
    let build_gltf_mesh = |_mesh_idx: usize,
                           mesh: &MapMesh,
                           root: &mut json::Root,
                           bin_data: &mut Vec<u8>,
                           mat_cache: &mut MapMaterialCache,
                           gltf_texture_cache: &HashMap<usize, json::Index<json::Texture>>|
     -> json::Index<json::Mesh> {
        let prim = build_map_mesh_primitive(root, bin_data, mesh, mat_cache, gltf_texture_cache);
        root.push(json::Mesh {
            primitives: vec![prim],
            weights: None,
            name: Some(mesh.name.clone()),
            extensions: Default::default(),
            extras: Default::default(),
        })
    };

    // Export model instances.
    for (i, inst) in scene.model_instances.iter().enumerate() {
        // Collect glTF meshes for this instance's model.
        let mut instance_meshes = Vec::new();
        for mesh_idx in inst.mesh_range.clone() {
            let gltf_mesh = if let Some(&cached) = gltf_mesh_cache.get(&mesh_idx) {
                cached
            } else {
                let mesh = &scene.model_meshes[mesh_idx];
                let m = build_gltf_mesh(mesh_idx, mesh, &mut root, &mut bin_data, &mut mat_cache, &gltf_texture_cache);
                gltf_mesh_cache.insert(mesh_idx, m);
                m
            };
            instance_meshes.push(gltf_mesh);
        }

        if instance_meshes.is_empty() {
            continue;
        }

        // If the model has a single mesh, create one node with the transform.
        // If multiple, create a parent node with children.
        if instance_meshes.len() == 1 {
            let node = root.push(json::Node {
                mesh: Some(instance_meshes[0]),
                name: Some(format!("Instance_{i}")),
                matrix: Some(inst.transform),
                ..Default::default()
            });
            scene_nodes.push(node);
        } else {
            let children: Vec<json::Index<json::Node>> = instance_meshes
                .iter()
                .enumerate()
                .map(|(j, &mesh)| {
                    root.push(json::Node {
                        mesh: Some(mesh),
                        name: Some(format!("Instance_{i}_part_{j}")),
                        ..Default::default()
                    })
                })
                .collect();
            let parent = root.push(json::Node {
                children: Some(children),
                name: Some(format!("Instance_{i}")),
                matrix: Some(inst.transform),
                ..Default::default()
            });
            scene_nodes.push(parent);
        }
    }

    // Export terrain mesh.
    if let Some(terrain) = &scene.terrain {
        let prim = build_map_mesh_primitive(&mut root, &mut bin_data, terrain, &mut mat_cache, &gltf_texture_cache);
        let gltf_mesh = root.push(json::Mesh {
            primitives: vec![prim],
            weights: None,
            name: Some("Terrain".to_string()),
            extensions: Default::default(),
            extras: Default::default(),
        });
        let node =
            root.push(json::Node { mesh: Some(gltf_mesh), name: Some("Terrain".to_string()), ..Default::default() });
        scene_nodes.push(node);
    }

    // Export water plane.
    if let Some(water) = &scene.water {
        let prim = build_map_mesh_primitive(&mut root, &mut bin_data, water, &mut mat_cache, &gltf_texture_cache);
        let gltf_mesh = root.push(json::Mesh {
            primitives: vec![prim],
            weights: None,
            name: Some("Water".to_string()),
            extensions: Default::default(),
            extras: Default::default(),
        });
        let node =
            root.push(json::Node { mesh: Some(gltf_mesh), name: Some("Water".to_string()), ..Default::default() });
        scene_nodes.push(node);
    }

    // Export vegetation instances (one node per instance, sharing cached meshes).
    for (mesh_idx, positions) in &scene.vegetation_instances {
        let gltf_mesh = if let Some(&cached) = gltf_mesh_cache.get(mesh_idx) {
            cached
        } else {
            let mesh = &scene.model_meshes[*mesh_idx];
            let m = build_gltf_mesh(*mesh_idx, mesh, &mut root, &mut bin_data, &mut mat_cache, &gltf_texture_cache);
            gltf_mesh_cache.insert(*mesh_idx, m);
            m
        };

        for (i, pos) in positions.iter().enumerate() {
            let node = root.push(json::Node {
                mesh: Some(gltf_mesh),
                name: Some(format!("Tree_{mesh_idx}_{i}")),
                translation: Some(*pos),
                ..Default::default()
            });
            scene_nodes.push(node);
        }
    }

    // Finalize GLB.
    while !bin_data.len().is_multiple_of(4) {
        bin_data.push(0);
    }

    if !bin_data.is_empty() {
        let buffer = root.push(json::Buffer {
            byte_length: USize64::from(bin_data.len()),
            uri: None,
            name: None,
            extensions: Default::default(),
            extras: Default::default(),
        });
        for bv in root.buffer_views.iter_mut() {
            bv.buffer = buffer;
        }
    }

    let scene = root.push(json::Scene {
        nodes: scene_nodes,
        name: None,
        extensions: Default::default(),
        extras: Default::default(),
    });
    root.scene = Some(scene);

    let json_string =
        json::serialize::to_string(&root).map_err(|e| Report::new(ExportError::Serialize(e.to_string())))?;

    let glb = gltf::binary::Glb {
        header: gltf::binary::Header { magic: *b"glTF", version: 2, length: 0 },
        json: Cow::Owned(json_string.into_bytes()),
        bin: if bin_data.is_empty() { None } else { Some(Cow::Owned(bin_data)) },
    };

    glb.to_writer(writer).map_err(|e| Report::new(ExportError::Io(e.to_string())))?;

    Ok(())
}

/// Build a single glTF primitive from a [`MapMesh`].
fn build_map_mesh_primitive(
    root: &mut json::Root,
    bin_data: &mut Vec<u8>,
    mesh: &MapMesh,
    mat_cache: &mut MapMaterialCache,
    gltf_texture_cache: &HashMap<usize, json::Index<json::Texture>>,
) -> json::mesh::Primitive {
    let mut attributes = BTreeMap::new();

    // --- Positions ---
    let pos_accessor = if !mesh.positions.is_empty() {
        let (min, max) = bounding_coords(&mesh.positions);
        let byte_offset = bin_data.len();
        for pos in &mesh.positions {
            bin_data.extend_from_slice(&pos[0].to_le_bytes());
            bin_data.extend_from_slice(&pos[1].to_le_bytes());
            bin_data.extend_from_slice(&pos[2].to_le_bytes());
        }
        pad_to_4(bin_data);
        let byte_length = bin_data.len() - byte_offset;

        let bv = root.push(json::buffer::View {
            buffer: json::Index::new(0),
            byte_length: USize64::from(byte_length),
            byte_offset: Some(USize64::from(byte_offset)),
            byte_stride: None,
            target: Some(Valid(json::buffer::Target::ArrayBuffer)),
            name: None,
            extensions: Default::default(),
            extras: Default::default(),
        });

        Some(root.push(json::Accessor {
            buffer_view: Some(bv),
            byte_offset: Some(USize64(0)),
            count: USize64::from(mesh.positions.len()),
            component_type: Valid(json::accessor::GenericComponentType(json::accessor::ComponentType::F32)),
            type_: Valid(json::accessor::Type::Vec3),
            min: Some(json::Value::from(min.to_vec())),
            max: Some(json::Value::from(max.to_vec())),
            name: None,
            normalized: false,
            sparse: None,
            extensions: Default::default(),
            extras: Default::default(),
        }))
    } else {
        None
    };

    // --- Normals ---
    let norm_accessor = if !mesh.normals.is_empty() {
        let byte_offset = bin_data.len();
        for n in &mesh.normals {
            bin_data.extend_from_slice(&n[0].to_le_bytes());
            bin_data.extend_from_slice(&n[1].to_le_bytes());
            bin_data.extend_from_slice(&n[2].to_le_bytes());
        }
        pad_to_4(bin_data);
        let byte_length = bin_data.len() - byte_offset;

        let bv = root.push(json::buffer::View {
            buffer: json::Index::new(0),
            byte_length: USize64::from(byte_length),
            byte_offset: Some(USize64::from(byte_offset)),
            byte_stride: None,
            target: Some(Valid(json::buffer::Target::ArrayBuffer)),
            name: None,
            extensions: Default::default(),
            extras: Default::default(),
        });

        Some(root.push(json::Accessor {
            buffer_view: Some(bv),
            byte_offset: Some(USize64(0)),
            count: USize64::from(mesh.normals.len()),
            component_type: Valid(json::accessor::GenericComponentType(json::accessor::ComponentType::F32)),
            type_: Valid(json::accessor::Type::Vec3),
            min: None,
            max: None,
            name: None,
            normalized: false,
            sparse: None,
            extensions: Default::default(),
            extras: Default::default(),
        }))
    } else {
        None
    };

    // --- UVs ---
    let uv_accessor = if !mesh.uvs.is_empty() {
        let byte_offset = bin_data.len();
        for uv in &mesh.uvs {
            bin_data.extend_from_slice(&uv[0].to_le_bytes());
            bin_data.extend_from_slice(&uv[1].to_le_bytes());
        }
        pad_to_4(bin_data);
        let byte_length = bin_data.len() - byte_offset;

        let bv = root.push(json::buffer::View {
            buffer: json::Index::new(0),
            byte_length: USize64::from(byte_length),
            byte_offset: Some(USize64::from(byte_offset)),
            byte_stride: None,
            target: Some(Valid(json::buffer::Target::ArrayBuffer)),
            name: None,
            extensions: Default::default(),
            extras: Default::default(),
        });

        Some(root.push(json::Accessor {
            buffer_view: Some(bv),
            byte_offset: Some(USize64(0)),
            count: USize64::from(mesh.uvs.len()),
            component_type: Valid(json::accessor::GenericComponentType(json::accessor::ComponentType::F32)),
            type_: Valid(json::accessor::Type::Vec2),
            min: None,
            max: None,
            name: None,
            normalized: false,
            sparse: None,
            extensions: Default::default(),
            extras: Default::default(),
        }))
    } else {
        None
    };

    // --- Indices ---
    let indices_accessor = if !mesh.indices.is_empty() {
        let byte_offset = bin_data.len();
        for &idx in &mesh.indices {
            bin_data.extend_from_slice(&idx.to_le_bytes());
        }
        pad_to_4(bin_data);
        let byte_length = bin_data.len() - byte_offset;

        let bv = root.push(json::buffer::View {
            buffer: json::Index::new(0),
            byte_length: USize64::from(byte_length),
            byte_offset: Some(USize64::from(byte_offset)),
            byte_stride: None,
            target: Some(Valid(json::buffer::Target::ElementArrayBuffer)),
            name: None,
            extensions: Default::default(),
            extras: Default::default(),
        });

        Some(root.push(json::Accessor {
            buffer_view: Some(bv),
            byte_offset: Some(USize64(0)),
            count: USize64::from(mesh.indices.len()),
            component_type: Valid(json::accessor::GenericComponentType(json::accessor::ComponentType::U32)),
            type_: Valid(json::accessor::Type::Scalar),
            min: None,
            max: None,
            name: None,
            normalized: false,
            sparse: None,
            extensions: Default::default(),
            extras: Default::default(),
        }))
    } else {
        None
    };

    // Build attribute map.
    if let Some(pos) = pos_accessor {
        attributes.insert(Valid(json::mesh::Semantic::Positions), pos);
    }
    if let Some(norm) = norm_accessor {
        attributes.insert(Valid(json::mesh::Semantic::Normals), norm);
    }
    if let Some(uv) = uv_accessor {
        attributes.insert(Valid(json::mesh::Semantic::TexCoords(0)), uv);
    }

    // Material: deduplicate by (texture index, base color, alpha blend, alpha cutoff).
    // Encode base_color as [u32; 4] for HashMap key (f32 isn't Hash).
    let mat_key = MapMaterialKey {
        albedo_texture: mesh.albedo_texture,
        base_color_bits: mesh.base_color.map(|c| c.to_bits()),
        alpha_blend: mesh.alpha_blend,
        alpha_cutoff_bits: mesh.alpha_cutoff.map(|c| c.to_bits()),
    };

    let material = *mat_cache.entry(mat_key).or_insert_with(|| {
        if let Some(tex_idx) = mesh.albedo_texture
            && let Some(&gltf_tex) = gltf_texture_cache.get(&tex_idx)
        {
            // Textured material: reference the shared glTF Texture.
            let (alpha_mode, alpha_cutoff_val, double_sided) = if let Some(cutoff) = mesh.alpha_cutoff {
                (Valid(json::material::AlphaMode::Mask), Some(json::material::AlphaCutoff(cutoff)), true)
            } else {
                (Valid(json::material::AlphaMode::Opaque), None, false)
            };
            root.push(json::Material {
                name: Some(mesh.name.clone()),
                pbr_metallic_roughness: json::material::PbrMetallicRoughness {
                    base_color_texture: Some(json::texture::Info {
                        index: gltf_tex,
                        tex_coord: 0,
                        extensions: Default::default(),
                        extras: Default::default(),
                    }),
                    base_color_factor: json::material::PbrBaseColorFactor([1.0, 1.0, 1.0, 1.0]),
                    ..Default::default()
                },
                alpha_mode,
                alpha_cutoff: alpha_cutoff_val,
                double_sided,
                ..Default::default()
            })
        } else if mesh.alpha_blend {
            root.push(json::Material {
                name: Some(mesh.name.clone()),
                pbr_metallic_roughness: json::material::PbrMetallicRoughness {
                    base_color_factor: json::material::PbrBaseColorFactor(mesh.base_color),
                    ..Default::default()
                },
                alpha_mode: Valid(json::material::AlphaMode::Blend),
                ..Default::default()
            })
        } else {
            root.push(json::Material {
                name: Some(mesh.name.clone()),
                pbr_metallic_roughness: json::material::PbrMetallicRoughness {
                    base_color_factor: json::material::PbrBaseColorFactor(mesh.base_color),
                    ..Default::default()
                },
                ..Default::default()
            })
        }
    });

    json::mesh::Primitive {
        attributes,
        indices: indices_accessor,
        material: Some(material),
        mode: Valid(json::mesh::Mode::Triangles),
        targets: None,
        extensions: Default::default(),
        extras: Default::default(),
    }
}

/// Export all models from a `models.bin` + `models.geometry` pair to a single GLB.
///
/// When `space` is provided, each instance in `space.bin` becomes a separate node
/// with the world transform applied. The same mesh is reused across instances that
/// share the same model prototype. When `space` is `None`, one node per prototype
/// is created at the origin (no transforms).
pub fn export_merged_models_glb(
    merged: &MergedModels,
    geometry: &MergedGeometry,
    space: Option<&SpaceInstances>,
    db: Option<&PrototypeDatabase<'_>>,
    lod: usize,
    writer: &mut impl Write,
) -> Result<(), Report<ExportError>> {
    let mut root = json::Root {
        asset: json::Asset {
            version: "2.0".to_string(),
            generator: Some("wowsunpack".to_string()),
            ..Default::default()
        },
        ..Default::default()
    };

    let mut bin_data: Vec<u8> = Vec::new();
    let mut mat_cache = MaterialCache::new();
    let empty_textures = TextureSet::empty();
    let mut scene_nodes = Vec::new();
    let mut exported = 0usize;

    // Precompute self_id_index once for all models (expensive to rebuild per-RS).
    let self_id_index = db.map(|db| db.build_self_id_index());

    // Build path_id → model index map for instance lookups.
    let path_to_model: HashMap<u64, usize> = merged.models.iter().enumerate().map(|(i, r)| (r.path_id, i)).collect();

    // Build one mesh per prototype, lazily (only when referenced by an instance).
    // Cache: model_index → glTF Mesh index.
    let mut mesh_cache: HashMap<usize, json::Index<json::Mesh>> = HashMap::new();

    let build_mesh = |model_idx: usize,
                      root: &mut json::Root,
                      bin_data: &mut Vec<u8>,
                      mat_cache: &mut MaterialCache|
     -> Result<Option<json::Index<json::Mesh>>, Report<ExportError>> {
        let record = &merged.models[model_idx];
        let vp = &record.visual_proto;

        if vp.lods.is_empty() || lod >= vp.lods.len() {
            return Ok(None);
        }

        let lod_entry = &vp.lods[lod];
        let primitives = match collect_primitives(vp, geometry, db, self_id_index.as_ref(), lod_entry, false, None) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("Warning: model[{model_idx}]: {e}");
                return Ok(None);
            }
        };

        if primitives.is_empty() {
            return Ok(None);
        }

        let mut gltf_primitives = Vec::new();
        for prim in &primitives {
            let gltf_prim = add_primitive_to_root(root, bin_data, prim, &empty_textures, mat_cache)?;
            gltf_primitives.push(gltf_prim);
        }

        let name = format!("Model_{model_idx}");
        let mesh = root.push(json::Mesh {
            primitives: gltf_primitives,
            weights: None,
            name: Some(name),
            extensions: Default::default(),
            extras: Default::default(),
        });

        Ok(Some(mesh))
    };

    if let Some(space) = space {
        // Instance mode: one node per space.bin instance with world transform.
        for (i, inst) in space.instances.iter().enumerate() {
            let Some(&model_idx) = path_to_model.get(&inst.path_id) else {
                continue;
            };

            let mesh = if let Some(&cached) = mesh_cache.get(&model_idx) {
                cached
            } else {
                match build_mesh(model_idx, &mut root, &mut bin_data, &mut mat_cache)? {
                    Some(m) => {
                        mesh_cache.insert(model_idx, m);
                        m
                    }
                    None => continue,
                }
            };

            // Apply world transform. glTF uses column-major 4×4 matrices.
            let node = root.push(json::Node {
                mesh: Some(mesh),
                name: Some(format!("Instance_{i}")),
                matrix: Some(inst.transform.0),
                ..Default::default()
            });

            scene_nodes.push(node);
            exported += 1;
        }
    } else {
        // Prototype mode: one node per model at origin (no transforms).
        for (i, _record) in merged.models.iter().enumerate() {
            let Some(mesh) = build_mesh(i, &mut root, &mut bin_data, &mut mat_cache)? else {
                continue;
            };

            let node =
                root.push(json::Node { mesh: Some(mesh), name: Some(format!("Model_{i}")), ..Default::default() });

            scene_nodes.push(node);
            exported += 1;
        }
    }

    if exported == 0 {
        eprintln!("Warning: no models exported for LOD {lod}");
    }

    // Pad binary data to 4-byte alignment.
    while !bin_data.len().is_multiple_of(4) {
        bin_data.push(0);
    }

    // Set the buffer byte_length.
    if !bin_data.is_empty() {
        let buffer = root.push(json::Buffer {
            byte_length: USize64::from(bin_data.len()),
            uri: None,
            name: None,
            extensions: Default::default(),
            extras: Default::default(),
        });
        for bv in root.buffer_views.iter_mut() {
            bv.buffer = buffer;
        }
    }

    let scene = root.push(json::Scene {
        nodes: scene_nodes,
        name: None,
        extensions: Default::default(),
        extras: Default::default(),
    });
    root.scene = Some(scene);

    let json_string =
        json::serialize::to_string(&root).map_err(|e| Report::new(ExportError::Serialize(e.to_string())))?;

    let glb = gltf::binary::Glb {
        header: gltf::binary::Header { magic: *b"glTF", version: 2, length: 0 },
        json: Cow::Owned(json_string.into_bytes()),
        bin: if bin_data.is_empty() { None } else { Some(Cow::Owned(bin_data)) },
    };

    glb.to_writer(writer).map_err(|e| Report::new(ExportError::Io(e.to_string())))?;

    Ok(())
}

/// Export raw geometry to GLB without a visual file.
///
/// Pairs `vertices_mapping[i]` with `indices_mapping[i]` by array index. Each
/// pair becomes a separate glTF primitive. No material names, textures, or LOD
/// filtering are available without the visual.
pub fn export_geometry_raw(geometry: &MergedGeometry, writer: &mut impl Write) -> Result<(), Report<ExportError>> {
    let pair_count = geometry.vertices_mapping.len().min(geometry.indices_mapping.len());

    if geometry.vertices_mapping.len() != geometry.indices_mapping.len() {
        eprintln!(
            "Warning: {} vertex mappings vs {} index mappings; exporting {} pairs",
            geometry.vertices_mapping.len(),
            geometry.indices_mapping.len(),
            pair_count,
        );
    }

    if pair_count == 0 {
        eprintln!("Warning: no mapping entries found; producing empty GLB");
    }

    let mut root = json::Root {
        asset: json::Asset {
            version: "2.0".to_string(),
            generator: Some("wowsunpack".to_string()),
            ..Default::default()
        },
        ..Default::default()
    };

    let mut bin_data: Vec<u8> = Vec::new();
    let mut gltf_primitives = Vec::new();

    for i in 0..pair_count {
        let vert_mapping = &geometry.vertices_mapping[i];
        let idx_mapping = &geometry.indices_mapping[i];

        // Get vertex buffer.
        let vbuf_idx = vert_mapping.merged_buffer_index as usize;
        if vbuf_idx >= geometry.merged_vertices.len() {
            eprintln!("Warning: primitive {i}: vertex buffer index {vbuf_idx} out of range, skipping");
            continue;
        }
        let vert_proto = &geometry.merged_vertices[vbuf_idx];

        // Get index buffer.
        let ibuf_idx = idx_mapping.merged_buffer_index as usize;
        if ibuf_idx >= geometry.merged_indices.len() {
            eprintln!("Warning: primitive {i}: index buffer index {ibuf_idx} out of range, skipping");
            continue;
        }
        let idx_proto = &geometry.merged_indices[ibuf_idx];

        // Decode buffers.
        let decoded_vertices = match vert_proto.data.decode() {
            Ok(v) => v,
            Err(e) => {
                eprintln!("Warning: primitive {i}: vertex decode error: {e:?}, skipping");
                continue;
            }
        };
        let decoded_indices = match idx_proto.data.decode() {
            Ok(v) => v,
            Err(e) => {
                eprintln!("Warning: primitive {i}: index decode error: {e:?}, skipping");
                continue;
            }
        };

        // Parse vertex format.
        let format = vertex_format::parse_vertex_format(&vert_proto.format_name);
        let stride = vert_proto.stride_in_bytes as usize;

        // Extract vertex slice.
        let vert_offset = vert_mapping.items_offset as usize;
        let vert_count = vert_mapping.items_count as usize;
        let vert_start = vert_offset * stride;
        let vert_end = vert_start + vert_count * stride;

        if vert_end > decoded_vertices.len() {
            eprintln!(
                "Warning: primitive {i}: vertex range {vert_start}..{vert_end} exceeds buffer size {}, skipping",
                decoded_vertices.len()
            );
            continue;
        }
        let vert_slice = &decoded_vertices[vert_start..vert_end];

        // Extract index slice.
        let idx_offset = idx_mapping.items_offset as usize;
        let idx_count = idx_mapping.items_count as usize;
        let index_size = idx_proto.index_size as usize;
        let idx_start = idx_offset * index_size;
        let idx_end = idx_start + idx_count * index_size;

        if idx_end > decoded_indices.len() {
            eprintln!(
                "Warning: primitive {i}: index range {idx_start}..{idx_end} exceeds buffer size {}, skipping",
                decoded_indices.len()
            );
            continue;
        }
        let idx_slice = &decoded_indices[idx_start..idx_end];

        // Parse indices as u32.
        let indices: Vec<u32> = match index_size {
            2 => idx_slice.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]]) as u32).collect(),
            4 => idx_slice.chunks_exact(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect(),
            _ => {
                eprintln!("Warning: primitive {i}: unsupported index size {index_size}, skipping");
                continue;
            }
        };

        let verts = unpack_vertices(vert_slice, stride, &format);

        // Build a DecodedPrimitive for reuse with add_primitive_to_root.
        let prim = DecodedPrimitive {
            positions: verts.positions,
            normals: verts.normals,
            uvs: verts.uvs,
            indices,
            material_name: format!("Primitive_{i}"),
            mfm_stem: None,
            mfm_full_path: None,
            mfm_path_id: 0,
        };

        let empty_textures = TextureSet::empty();
        let mut mat_cache = MaterialCache::new();
        let gltf_prim = add_primitive_to_root(&mut root, &mut bin_data, &prim, &empty_textures, &mut mat_cache)?;
        gltf_primitives.push(gltf_prim);
    }

    // Pad binary data to 4-byte alignment.
    while !bin_data.len().is_multiple_of(4) {
        bin_data.push(0);
    }

    // Set the buffer byte_length.
    if !bin_data.is_empty() {
        let buffer = root.push(json::Buffer {
            byte_length: USize64::from(bin_data.len()),
            uri: None,
            name: None,
            extensions: Default::default(),
            extras: Default::default(),
        });
        for bv in root.buffer_views.iter_mut() {
            bv.buffer = buffer;
        }
    }

    // Create mesh with all primitives.
    let mesh = root.push(json::Mesh {
        primitives: gltf_primitives,
        weights: None,
        name: None,
        extensions: Default::default(),
        extras: Default::default(),
    });

    let root_node = root.push(json::Node { mesh: Some(mesh), ..Default::default() });

    let scene = root.push(json::Scene {
        nodes: vec![root_node],
        name: None,
        extensions: Default::default(),
        extras: Default::default(),
    });
    root.scene = Some(scene);

    // Serialize and write GLB.
    let json_string =
        json::serialize::to_string(&root).map_err(|e| Report::new(ExportError::Serialize(e.to_string())))?;

    let glb = gltf::binary::Glb {
        header: gltf::binary::Header { magic: *b"glTF", version: 2, length: 0 },
        json: Cow::Owned(json_string.into_bytes()),
        bin: if bin_data.is_empty() { None } else { Some(Cow::Owned(bin_data)) },
    };

    glb.to_writer(writer).map_err(|e| Report::new(ExportError::Io(e.to_string())))?;

    println!("  Exported {pair_count} raw primitives");
    Ok(())
}

/// Render set name substrings to exclude for intact-state export.
///
/// BigWorld ship visuals contain both intact and damaged geometry in the same
/// file. Crack geometry (`_crack_`) shows jagged fracture edges for the damaged
/// state, while patch geometry (`_patch_`) covers those seams when intact.
/// The `_hide` geometry is context-dependent and hidden by default.
const INTACT_EXCLUDE: &[&str] = &["_crack_", "_hide"];

/// Render set name substrings to exclude for damaged-state export.
///
/// In the damaged state, patch geometry is hidden and crack geometry is shown.
const DAMAGED_EXCLUDE: &[&str] = &["_patch_", "_hide"];

/// Collect and decode all render set primitives for a given LOD.
///
/// When `damaged` is false, crack and hide geometry is excluded (intact hull).
/// When `damaged` is true, patch and hide geometry is excluded (destroyed look).
fn collect_primitives(
    visual: &VisualPrototype,
    geometry: &MergedGeometry,
    db: Option<&PrototypeDatabase<'_>>,
    self_id_index: Option<&HashMap<u64, usize>>,
    lod: &crate::models::visual::Lod,
    damaged: bool,
    barrel_pitch: Option<&BarrelPitch>,
) -> Result<Vec<DecodedPrimitive>, Report<ExportError>> {
    let mut result = Vec::new();
    let exclude = if damaged { DAMAGED_EXCLUDE } else { INTACT_EXCLUDE };

    for &rs_name_id in &lod.render_set_names {
        // Find the render set with this name_id.
        let rs = visual
            .render_sets
            .iter()
            .find(|rs| rs.name_id == rs_name_id)
            .ok_or_else(|| Report::new(ExportError::RenderSetNotFound(rs_name_id)))?;

        // Skip render sets based on damage state (requires string table).
        if let Some(db) = db
            && let Some(rs_name) = db.strings.get_string_by_id(rs_name_id)
            && exclude.iter().any(|sub| rs_name.contains(sub))
        {
            continue;
        }

        let vertices_mapping_id = rs.vertices_mapping_id;
        let indices_mapping_id = rs.indices_mapping_id;

        // Find mapping entries.
        let vert_mapping = geometry
            .vertices_mapping
            .iter()
            .find(|m| m.mapping_id == vertices_mapping_id)
            .ok_or_else(|| Report::new(ExportError::VerticesMappingNotFound { id: vertices_mapping_id }))?;

        let idx_mapping = geometry
            .indices_mapping
            .iter()
            .find(|m| m.mapping_id == indices_mapping_id)
            .ok_or_else(|| Report::new(ExportError::IndicesMappingNotFound { id: indices_mapping_id }))?;

        // Get vertex buffer.
        let vbuf_idx = vert_mapping.merged_buffer_index as usize;
        if vbuf_idx >= geometry.merged_vertices.len() {
            return Err(Report::new(ExportError::BufferIndexOutOfRange {
                index: vbuf_idx,
                count: geometry.merged_vertices.len(),
            }));
        }
        let vert_proto = &geometry.merged_vertices[vbuf_idx];

        // Get index buffer.
        let ibuf_idx = idx_mapping.merged_buffer_index as usize;
        if ibuf_idx >= geometry.merged_indices.len() {
            return Err(Report::new(ExportError::BufferIndexOutOfRange {
                index: ibuf_idx,
                count: geometry.merged_indices.len(),
            }));
        }
        let idx_proto = &geometry.merged_indices[ibuf_idx];

        // Decode buffers.
        let decoded_vertices =
            vert_proto.data.decode().map_err(|e| Report::new(ExportError::VertexDecode(format!("{e:?}"))))?;
        let decoded_indices =
            idx_proto.data.decode().map_err(|e| Report::new(ExportError::IndexDecode(format!("{e:?}"))))?;

        // Parse vertex format.
        let format = vertex_format::parse_vertex_format(&vert_proto.format_name);
        let stride = vert_proto.stride_in_bytes as usize;

        if format.stride != stride {
            // The parsed format stride doesn't match the geometry's stride.
            // This can happen for formats we don't fully parse. Use the
            // geometry stride and just extract what we can.
            eprintln!(
                "Warning: format \"{}\" parsed stride {} != geometry stride {}; using geometry stride",
                vert_proto.format_name, format.stride, stride
            );
        }

        // Extract vertex slice.
        let vert_offset = vert_mapping.items_offset as usize;
        let vert_count = vert_mapping.items_count as usize;
        let vert_start = vert_offset * stride;
        let vert_end = vert_start + vert_count * stride;

        if vert_end > decoded_vertices.len() {
            return Err(Report::new(ExportError::VertexDecode(format!(
                "vertex range {}..{} exceeds buffer size {}",
                vert_start,
                vert_end,
                decoded_vertices.len()
            ))));
        }
        let vert_slice = &decoded_vertices[vert_start..vert_end];

        // Extract index slice.
        let idx_offset = idx_mapping.items_offset as usize;
        let idx_count = idx_mapping.items_count as usize;
        let index_size = idx_proto.index_size as usize;
        let idx_start = idx_offset * index_size;
        let idx_end = idx_start + idx_count * index_size;

        if idx_end > decoded_indices.len() {
            return Err(Report::new(ExportError::IndexDecode(format!(
                "index range {}..{} exceeds buffer size {}",
                idx_start,
                idx_end,
                decoded_indices.len()
            ))));
        }
        let idx_slice = &decoded_indices[idx_start..idx_end];

        // Parse indices as u32.
        let indices: Vec<u32> = match index_size {
            2 => idx_slice.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]]) as u32).collect(),
            4 => idx_slice.chunks_exact(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect(),
            _ => {
                return Err(Report::new(ExportError::IndexDecode(format!("unsupported index size: {index_size}"))));
            }
        };

        // Indices are already 0-based relative to the vertex slice
        // (items_offset is applied when extracting the vertex slice).

        // Unpack vertex attributes.
        let mut verts = unpack_vertices(vert_slice, stride, &format);

        // Apply per-vertex barrel pitch rotation if configured.
        if let Some(bp) = barrel_pitch {
            apply_barrel_pitch(&mut verts.positions, &mut verts.normals, vert_slice, stride, &format, bp);
        }

        // Material name for this render set.
        let material_name = db
            .and_then(|db| db.strings.get_string_by_id(rs.material_name_id))
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("material_0x{:08X}", rs.material_name_id));

        // Resolve MFM stem + full path for texture lookup (requires db + self_id_index).
        let (mfm_stem, mfm_full_path) = if rs.material_mfm_path_id != 0 {
            self_id_index
                .and_then(|idx_map| idx_map.get(&rs.material_mfm_path_id))
                .and_then(|&idx| {
                    db.map(|db| {
                        let full_path = db.reconstruct_path(idx, self_id_index.unwrap());
                        let leaf = &db.paths_storage[idx].name;
                        let stem = leaf.strip_suffix(".mfm").unwrap_or(leaf).to_string();
                        (Some(stem), Some(full_path))
                    })
                })
                .unwrap_or((None, None))
        } else {
            (None, None)
        };

        result.push(DecodedPrimitive {
            positions: verts.positions,
            normals: verts.normals,
            uvs: verts.uvs,
            indices,
            material_name,
            mfm_stem,
            mfm_full_path,
            mfm_path_id: rs.material_mfm_path_id,
        });
    }

    Ok(result)
}

struct UnpackedVertices {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
}

/// Unpack vertex data into separate position, normal, and UV arrays.
fn unpack_vertices(data: &[u8], stride: usize, format: &VertexFormat) -> UnpackedVertices {
    let count = data.len() / stride;
    let mut positions = Vec::with_capacity(count);
    let mut normals = Vec::with_capacity(count);
    let mut uvs = Vec::with_capacity(count);

    // Find attribute offsets.
    let pos_attr = format.attributes.iter().find(|a| a.semantic == AttributeSemantic::Position);
    let norm_attr = format.attributes.iter().find(|a| a.semantic == AttributeSemantic::Normal);
    let uv_attr = format.attributes.iter().find(|a| a.semantic == AttributeSemantic::TexCoord0);

    for i in 0..count {
        let base = i * stride;

        // Position: 3 x f32
        if let Some(attr) = pos_attr {
            let off = base + attr.offset;
            let x = f32::from_le_bytes(data[off..off + 4].try_into().unwrap());
            let y = f32::from_le_bytes(data[off + 4..off + 8].try_into().unwrap());
            let z = f32::from_le_bytes(data[off + 8..off + 12].try_into().unwrap());
            // Negate Z: converts left-handed (BigWorld) to right-handed (glTF).
            // This implicitly reverses triangle winding and flips normals consistently.
            positions.push([x, y, -z]);
        }

        // Normal: packed 4 bytes — negate Z to match position space.
        if let Some(attr) = norm_attr {
            let off = base + attr.offset;
            let packed = u32::from_le_bytes(data[off..off + 4].try_into().unwrap());
            let [nx, ny, nz] = vertex_format::unpack_normal(packed);
            normals.push([nx, ny, -nz]);
        }

        // UV: packed 4 bytes (2 x float16)
        if let Some(attr) = uv_attr {
            let off = base + attr.offset;
            let packed = u32::from_le_bytes(data[off..off + 4].try_into().unwrap());
            uvs.push(vertex_format::unpack_uv(packed));
        }
    }

    UnpackedVertices { positions, normals, uvs }
}

/// Convert a column-major 4x4 transform from left-handed to right-handed
/// coordinates by conjugating with S = diag(1,1,-1,1): M' = S * M * S.
/// This negates the Z row and Z column of the 3x3 rotation, plus the Z translation.
pub(super) fn negate_z_transform(m: [f32; 16]) -> [f32; 16] {
    [
        m[0], m[1], -m[2], m[3], // col 0: negate row 2
        m[4], m[5], -m[6], m[7], // col 1: negate row 2
        -m[8], -m[9], m[10], m[11], // col 2: negate col 2, but row 2 double-negates
        m[12], m[13], -m[14], m[15], // col 3: negate Z translation
    ]
}

/// Apply a pitch rotation to vertices whose dominant bone is a barrel bone.
///
/// Reads bone indices from the raw vertex data (the `iiiww` format: 3 u8 bone
/// indices + 1 padding byte, then 4 bytes of weights). The first bone index is
/// the dominant one. If it matches a barrel bone, the vertex position and normal
/// are transformed by the pitch matrix.
fn apply_barrel_pitch(
    positions: &mut [[f32; 3]],
    normals: &mut [[f32; 3]],
    vert_data: &[u8],
    stride: usize,
    format: &VertexFormat,
    bp: &BarrelPitch,
) {
    use crate::models::vertex_format::AttributeSemantic;

    let bone_idx_attr = format.attributes.iter().find(|a| a.semantic == AttributeSemantic::BoneIndices);
    let Some(bone_attr) = bone_idx_attr else {
        return; // No bone data — can't split
    };

    let m = &bp.pitch_matrix;
    let count = positions.len();
    for i in 0..count {
        let base = i * stride;
        let off = base + bone_attr.offset;
        // First byte is the dominant bone index.
        let dominant_bone = vert_data[off];
        if !bp.barrel_bone_indices.contains(&dominant_bone) {
            continue;
        }
        // Transform position: p' = M * p (column-major 4x4, affine)
        let [px, py, pz] = positions[i];
        positions[i] = [
            m[0] * px + m[4] * py + m[8] * pz + m[12],
            m[1] * px + m[5] * py + m[9] * pz + m[13],
            m[2] * px + m[6] * py + m[10] * pz + m[14],
        ];
        // Transform normal: n' = R * n (rotation only, no translation)
        if i < normals.len() {
            let [nx, ny, nz] = normals[i];
            normals[i] = [
                m[0] * nx + m[4] * ny + m[8] * nz,
                m[1] * nx + m[5] * ny + m[9] * nz,
                m[2] * nx + m[6] * ny + m[10] * nz,
            ];
        }
    }
}

/// All texture data for a ship export: base albedo + camouflage variants.
pub struct TextureSet {
    /// Base albedo PNGs keyed by MFM stem — the default ship appearance.
    pub base: HashMap<String, Vec<u8>>,
    /// Camouflage variant PNGs: scheme name → (MFM stem → PNG bytes).
    /// Only stems that have a texture for this scheme are included.
    pub camo_schemes: Vec<(String, HashMap<String, Vec<u8>>)>,
    /// UV scale/offset for tiled camo schemes. Key = `(scheme_index, mfm_stem)`.
    /// Only present for tiled camos; non-tiled camos use default UVs.
    pub tiled_uv_transforms: HashMap<(usize, String), [f32; 4]>,
}

impl TextureSet {
    pub fn empty() -> Self {
        Self { base: HashMap::new(), camo_schemes: Vec::new(), tiled_uv_transforms: HashMap::new() }
    }
}

/// Cached material info for a given MFM stem / material name.
struct CachedMaterial {
    /// Default material index (base albedo or untextured).
    default_mat: json::Index<json::Material>,
    /// Variant material indices, one per camo scheme (same order as TextureSet::camo_schemes).
    variant_mats: Vec<Option<json::Index<json::Material>>>,
}

/// Cache for deduplicating materials and textures across primitives.
struct MaterialCache {
    /// Maps cache key (MFM stem or material name) to cached material info.
    materials: HashMap<String, CachedMaterial>,
}

impl MaterialCache {
    fn new() -> Self {
        Self { materials: HashMap::new() }
    }
}

/// Embed a PNG image in the glTF binary buffer and create a textured material.
/// Returns the material index.
///
/// `uv_transform` is an optional `[scale_x, scale_y, offset_x, offset_y]` applied
/// via `KHR_texture_transform` for tiled camouflage textures.
fn create_textured_material(
    root: &mut json::Root,
    bin_data: &mut Vec<u8>,
    png_bytes: &[u8],
    material_name: &str,
    image_name: Option<String>,
    uv_transform: Option<[f32; 4]>,
) -> json::Index<json::Material> {
    let byte_offset = bin_data.len();
    bin_data.extend_from_slice(png_bytes);
    pad_to_4(bin_data);
    let byte_length = png_bytes.len();

    let bv = root.push(json::buffer::View {
        buffer: json::Index::new(0),
        byte_length: USize64::from(byte_length),
        byte_offset: Some(USize64::from(byte_offset)),
        byte_stride: None,
        target: None,
        name: None,
        extensions: Default::default(),
        extras: Default::default(),
    });

    let image = root.push(json::Image {
        buffer_view: Some(bv),
        mime_type: Some(json::image::MimeType("image/png".to_string())),
        uri: None,
        name: image_name,
        extensions: Default::default(),
        extras: Default::default(),
    });

    let sampler = root.push(json::texture::Sampler {
        mag_filter: Some(Valid(json::texture::MagFilter::Linear)),
        min_filter: Some(Valid(json::texture::MinFilter::LinearMipmapLinear)),
        wrap_s: Valid(json::texture::WrappingMode::Repeat),
        wrap_t: Valid(json::texture::WrappingMode::Repeat),
        name: None,
        extensions: Default::default(),
        extras: Default::default(),
    });

    let texture = root.push(json::Texture {
        source: image,
        sampler: Some(sampler),
        name: None,
        extensions: Default::default(),
        extras: Default::default(),
    });

    let tex_transform_ext = uv_transform.map(|t| json::extensions::texture::Info {
        texture_transform: Some(json::extensions::texture::TextureTransform {
            scale: json::extensions::texture::TextureTransformScale(t[0..2].try_into().unwrap()),
            offset: json::extensions::texture::TextureTransformOffset(t[2..4].try_into().unwrap()),
            rotation: Default::default(),
            tex_coord: Some(0),
            extras: Default::default(),
        }),
    });

    let texture_info =
        json::texture::Info { index: texture, tex_coord: 0, extensions: tex_transform_ext, extras: Default::default() };

    root.push(json::Material {
        name: Some(material_name.to_string()),
        pbr_metallic_roughness: json::material::PbrMetallicRoughness {
            base_color_texture: Some(texture_info),
            ..Default::default()
        },
        ..Default::default()
    })
}

/// Create an untextured material.
fn create_untextured_material(root: &mut json::Root, material_name: &str) -> json::Index<json::Material> {
    root.push(json::Material { name: Some(material_name.to_string()), ..Default::default() })
}

/// Add a decoded primitive's data to the glTF root and binary buffer.
/// Returns the glTF Primitive JSON object.
fn add_primitive_to_root(
    root: &mut json::Root,
    bin_data: &mut Vec<u8>,
    prim: &DecodedPrimitive,
    texture_set: &TextureSet,
    mat_cache: &mut MaterialCache,
) -> Result<json::mesh::Primitive, Report<ExportError>> {
    let mut attributes = BTreeMap::new();

    // --- Positions ---
    let pos_accessor = if !prim.positions.is_empty() {
        let (min, max) = bounding_coords(&prim.positions);
        let byte_offset = bin_data.len();
        for pos in &prim.positions {
            bin_data.extend_from_slice(&pos[0].to_le_bytes());
            bin_data.extend_from_slice(&pos[1].to_le_bytes());
            bin_data.extend_from_slice(&pos[2].to_le_bytes());
        }
        pad_to_4(bin_data);
        let byte_length = bin_data.len() - byte_offset;

        let bv = root.push(json::buffer::View {
            buffer: json::Index::new(0),
            byte_length: USize64::from(byte_length),
            byte_offset: Some(USize64::from(byte_offset)),
            byte_stride: None,
            target: Some(Valid(json::buffer::Target::ArrayBuffer)),
            name: None,
            extensions: Default::default(),
            extras: Default::default(),
        });

        Some(root.push(json::Accessor {
            buffer_view: Some(bv),
            byte_offset: Some(USize64(0)),
            count: USize64::from(prim.positions.len()),
            component_type: Valid(json::accessor::GenericComponentType(json::accessor::ComponentType::F32)),
            type_: Valid(json::accessor::Type::Vec3),
            min: Some(json::Value::from(min.to_vec())),
            max: Some(json::Value::from(max.to_vec())),
            name: None,
            normalized: false,
            sparse: None,
            extensions: Default::default(),
            extras: Default::default(),
        }))
    } else {
        None
    };

    // --- Normals ---
    let norm_accessor = if !prim.normals.is_empty() {
        let byte_offset = bin_data.len();
        for n in &prim.normals {
            bin_data.extend_from_slice(&n[0].to_le_bytes());
            bin_data.extend_from_slice(&n[1].to_le_bytes());
            bin_data.extend_from_slice(&n[2].to_le_bytes());
        }
        pad_to_4(bin_data);
        let byte_length = bin_data.len() - byte_offset;

        let bv = root.push(json::buffer::View {
            buffer: json::Index::new(0),
            byte_length: USize64::from(byte_length),
            byte_offset: Some(USize64::from(byte_offset)),
            byte_stride: None,
            target: Some(Valid(json::buffer::Target::ArrayBuffer)),
            name: None,
            extensions: Default::default(),
            extras: Default::default(),
        });

        Some(root.push(json::Accessor {
            buffer_view: Some(bv),
            byte_offset: Some(USize64(0)),
            count: USize64::from(prim.normals.len()),
            component_type: Valid(json::accessor::GenericComponentType(json::accessor::ComponentType::F32)),
            type_: Valid(json::accessor::Type::Vec3),
            min: None,
            max: None,
            name: None,
            normalized: false,
            sparse: None,
            extensions: Default::default(),
            extras: Default::default(),
        }))
    } else {
        None
    };

    // --- UVs ---
    let uv_accessor = if !prim.uvs.is_empty() {
        let byte_offset = bin_data.len();
        for uv in &prim.uvs {
            bin_data.extend_from_slice(&uv[0].to_le_bytes());
            bin_data.extend_from_slice(&uv[1].to_le_bytes());
        }
        pad_to_4(bin_data);
        let byte_length = bin_data.len() - byte_offset;

        let bv = root.push(json::buffer::View {
            buffer: json::Index::new(0),
            byte_length: USize64::from(byte_length),
            byte_offset: Some(USize64::from(byte_offset)),
            byte_stride: None,
            target: Some(Valid(json::buffer::Target::ArrayBuffer)),
            name: None,
            extensions: Default::default(),
            extras: Default::default(),
        });

        Some(root.push(json::Accessor {
            buffer_view: Some(bv),
            byte_offset: Some(USize64(0)),
            count: USize64::from(prim.uvs.len()),
            component_type: Valid(json::accessor::GenericComponentType(json::accessor::ComponentType::F32)),
            type_: Valid(json::accessor::Type::Vec2),
            min: None,
            max: None,
            name: None,
            normalized: false,
            sparse: None,
            extensions: Default::default(),
            extras: Default::default(),
        }))
    } else {
        None
    };

    // --- Indices ---
    let indices_accessor = if !prim.indices.is_empty() {
        let byte_offset = bin_data.len();
        for &idx in &prim.indices {
            bin_data.extend_from_slice(&idx.to_le_bytes());
        }
        pad_to_4(bin_data);
        let byte_length = bin_data.len() - byte_offset;

        let bv = root.push(json::buffer::View {
            buffer: json::Index::new(0),
            byte_length: USize64::from(byte_length),
            byte_offset: Some(USize64::from(byte_offset)),
            byte_stride: None,
            target: Some(Valid(json::buffer::Target::ElementArrayBuffer)),
            name: None,
            extensions: Default::default(),
            extras: Default::default(),
        });

        Some(root.push(json::Accessor {
            buffer_view: Some(bv),
            byte_offset: Some(USize64(0)),
            count: USize64::from(prim.indices.len()),
            component_type: Valid(json::accessor::GenericComponentType(json::accessor::ComponentType::U32)),
            type_: Valid(json::accessor::Type::Scalar),
            min: None,
            max: None,
            name: None,
            normalized: false,
            sparse: None,
            extensions: Default::default(),
            extras: Default::default(),
        }))
    } else {
        None
    };

    // Build attribute map.
    if let Some(pos) = pos_accessor {
        attributes.insert(Valid(json::mesh::Semantic::Positions), pos);
    }
    if let Some(norm) = norm_accessor {
        attributes.insert(Valid(json::mesh::Semantic::Normals), norm);
    }
    if let Some(uv) = uv_accessor {
        attributes.insert(Valid(json::mesh::Semantic::TexCoords(0)), uv);
    }

    // Determine cache key: prefer MFM stem, fall back to material name.
    let cache_key = prim.mfm_stem.clone().unwrap_or_else(|| prim.material_name.clone());

    if !mat_cache.materials.contains_key(&cache_key) {
        // Create the default material (base albedo or untextured).
        let default_mat = if let Some(png_bytes) = prim.mfm_stem.as_ref().and_then(|stem| texture_set.base.get(stem)) {
            create_textured_material(root, bin_data, png_bytes, &prim.material_name, prim.mfm_stem.clone(), None)
        } else {
            create_untextured_material(root, &prim.material_name)
        };

        // Create variant materials for each camo scheme.
        let variant_mats: Vec<Option<json::Index<json::Material>>> = texture_set
            .camo_schemes
            .iter()
            .enumerate()
            .map(|(scheme_idx, (scheme_name, scheme_textures))| {
                prim.mfm_stem.as_ref().and_then(|stem| {
                    let png_bytes = scheme_textures.get(stem)?;
                    let uv_xform = texture_set.tiled_uv_transforms.get(&(scheme_idx, stem.clone())).copied();
                    Some(create_textured_material(
                        root,
                        bin_data,
                        png_bytes,
                        &format!("{} [{}]", prim.material_name, scheme_name),
                        Some(format!("{stem}_{scheme_name}")),
                        uv_xform,
                    ))
                })
            })
            .collect();

        mat_cache.materials.insert(cache_key.clone(), CachedMaterial { default_mat, variant_mats });
    }

    let cached = &mat_cache.materials[&cache_key];

    // Build KHR_materials_variants mappings for this primitive.
    let prim_variants_ext = if !texture_set.camo_schemes.is_empty() {
        let mut mappings = Vec::new();
        for (variant_idx, variant_mat) in cached.variant_mats.iter().enumerate() {
            // Use the variant material if this stem has a camo texture for this scheme,
            // otherwise fall back to the default material.
            let mat_index = variant_mat.unwrap_or(cached.default_mat);
            mappings.push(json::extensions::mesh::Mapping {
                material: mat_index.value() as u32,
                variants: vec![variant_idx as u32],
            });
        }
        Some(json::extensions::mesh::KhrMaterialsVariants { mappings })
    } else {
        None
    };

    Ok(json::mesh::Primitive {
        attributes,
        indices: indices_accessor,
        material: Some(cached.default_mat),
        mode: Valid(json::mesh::Mode::Triangles),
        targets: None,
        extensions: Some(json::extensions::mesh::Primitive { khr_materials_variants: prim_variants_ext }),
        extras: Default::default(),
    })
}

/// Add an armor mesh primitive (positions + normals, untextured) to the glTF root.
fn add_armor_primitive_to_root(
    root: &mut json::Root,
    bin_data: &mut Vec<u8>,
    armor: &ArmorSubModel,
) -> Result<json::mesh::Primitive, Report<ExportError>> {
    let mut attributes = BTreeMap::new();

    // --- Positions ---
    let (min, max) = bounding_coords(&armor.positions);
    let byte_offset = bin_data.len();
    for pos in &armor.positions {
        bin_data.extend_from_slice(&pos[0].to_le_bytes());
        bin_data.extend_from_slice(&pos[1].to_le_bytes());
        bin_data.extend_from_slice(&pos[2].to_le_bytes());
    }
    pad_to_4(bin_data);
    let byte_length = bin_data.len() - byte_offset;

    let pos_bv = root.push(json::buffer::View {
        buffer: json::Index::new(0),
        byte_length: USize64::from(byte_length),
        byte_offset: Some(USize64::from(byte_offset)),
        byte_stride: None,
        target: Some(Valid(json::buffer::Target::ArrayBuffer)),
        name: None,
        extensions: Default::default(),
        extras: Default::default(),
    });

    let pos_acc = root.push(json::Accessor {
        buffer_view: Some(pos_bv),
        byte_offset: Some(USize64(0)),
        count: USize64::from(armor.positions.len()),
        component_type: Valid(json::accessor::GenericComponentType(json::accessor::ComponentType::F32)),
        type_: Valid(json::accessor::Type::Vec3),
        min: Some(json::Value::from(min.to_vec())),
        max: Some(json::Value::from(max.to_vec())),
        name: None,
        normalized: false,
        sparse: None,
        extensions: Default::default(),
        extras: Default::default(),
    });
    attributes.insert(Valid(json::mesh::Semantic::Positions), pos_acc);

    // --- Normals ---
    let byte_offset = bin_data.len();
    for n in &armor.normals {
        bin_data.extend_from_slice(&n[0].to_le_bytes());
        bin_data.extend_from_slice(&n[1].to_le_bytes());
        bin_data.extend_from_slice(&n[2].to_le_bytes());
    }
    pad_to_4(bin_data);
    let byte_length = bin_data.len() - byte_offset;

    let norm_bv = root.push(json::buffer::View {
        buffer: json::Index::new(0),
        byte_length: USize64::from(byte_length),
        byte_offset: Some(USize64::from(byte_offset)),
        byte_stride: None,
        target: Some(Valid(json::buffer::Target::ArrayBuffer)),
        name: None,
        extensions: Default::default(),
        extras: Default::default(),
    });

    let norm_acc = root.push(json::Accessor {
        buffer_view: Some(norm_bv),
        byte_offset: Some(USize64(0)),
        count: USize64::from(armor.normals.len()),
        component_type: Valid(json::accessor::GenericComponentType(json::accessor::ComponentType::F32)),
        type_: Valid(json::accessor::Type::Vec3),
        min: None,
        max: None,
        name: None,
        normalized: false,
        sparse: None,
        extensions: Default::default(),
        extras: Default::default(),
    });
    attributes.insert(Valid(json::mesh::Semantic::Normals), norm_acc);

    // --- Vertex Colors (COLOR_0) ---
    if !armor.colors.is_empty() {
        let byte_offset = bin_data.len();
        for c in &armor.colors {
            bin_data.extend_from_slice(&c[0].to_le_bytes());
            bin_data.extend_from_slice(&c[1].to_le_bytes());
            bin_data.extend_from_slice(&c[2].to_le_bytes());
            bin_data.extend_from_slice(&c[3].to_le_bytes());
        }
        pad_to_4(bin_data);
        let byte_length = bin_data.len() - byte_offset;

        let color_bv = root.push(json::buffer::View {
            buffer: json::Index::new(0),
            byte_length: USize64::from(byte_length),
            byte_offset: Some(USize64::from(byte_offset)),
            byte_stride: None,
            target: Some(Valid(json::buffer::Target::ArrayBuffer)),
            name: None,
            extensions: Default::default(),
            extras: Default::default(),
        });

        let color_acc = root.push(json::Accessor {
            buffer_view: Some(color_bv),
            byte_offset: Some(USize64(0)),
            count: USize64::from(armor.colors.len()),
            component_type: Valid(json::accessor::GenericComponentType(json::accessor::ComponentType::F32)),
            type_: Valid(json::accessor::Type::Vec4),
            min: None,
            max: None,
            name: None,
            normalized: false,
            sparse: None,
            extensions: Default::default(),
            extras: Default::default(),
        });
        attributes.insert(Valid(json::mesh::Semantic::Colors(0)), color_acc);
    }

    // --- Indices ---
    let byte_offset = bin_data.len();
    for &idx in &armor.indices {
        bin_data.extend_from_slice(&idx.to_le_bytes());
    }
    pad_to_4(bin_data);
    let byte_length = bin_data.len() - byte_offset;

    let idx_bv = root.push(json::buffer::View {
        buffer: json::Index::new(0),
        byte_length: USize64::from(byte_length),
        byte_offset: Some(USize64::from(byte_offset)),
        byte_stride: None,
        target: Some(Valid(json::buffer::Target::ElementArrayBuffer)),
        name: None,
        extensions: Default::default(),
        extras: Default::default(),
    });

    let idx_acc = root.push(json::Accessor {
        buffer_view: Some(idx_bv),
        byte_offset: Some(USize64(0)),
        count: USize64::from(armor.indices.len()),
        component_type: Valid(json::accessor::GenericComponentType(json::accessor::ComponentType::U32)),
        type_: Valid(json::accessor::Type::Scalar),
        min: None,
        max: None,
        name: None,
        normalized: false,
        sparse: None,
        extensions: Default::default(),
        extras: Default::default(),
    });

    // Untextured semi-transparent material for armor visualization.
    let material = root.push(json::Material {
        name: Some(format!("armor_{}", armor.name)),
        alpha_mode: Valid(json::material::AlphaMode::Blend),
        pbr_metallic_roughness: json::material::PbrMetallicRoughness {
            base_color_factor: json::material::PbrBaseColorFactor([1.0, 1.0, 1.0, 1.0]),
            metallic_factor: json::material::StrengthFactor(0.0),
            roughness_factor: json::material::StrengthFactor(0.8),
            ..Default::default()
        },
        double_sided: true,
        ..Default::default()
    });

    Ok(json::mesh::Primitive {
        attributes,
        indices: Some(idx_acc),
        material: Some(material),
        mode: Valid(json::mesh::Mode::Triangles),
        targets: None,
        extensions: None,
        extras: Default::default(),
    })
}

/// Add `KHR_materials_variants` root extension and `extensionsUsed` entry.
///
/// Creates variant definitions at the glTF root so that each camo scheme name
/// appears as a selectable variant in viewers like Blender.
fn add_variants_extension(root: &mut json::Root, texture_set: &TextureSet) {
    if texture_set.camo_schemes.is_empty() {
        return;
    }

    let variants: Vec<json::extensions::scene::khr_materials_variants::Variant> = texture_set
        .camo_schemes
        .iter()
        .map(|(name, _)| json::extensions::scene::khr_materials_variants::Variant { name: name.clone() })
        .collect();

    let ext = json::extensions::root::KhrMaterialsVariants { variants };
    root.extensions = Some(json::extensions::root::Root { khr_materials_variants: Some(ext) });

    root.extensions_used.push("KHR_materials_variants".to_string());

    if !texture_set.tiled_uv_transforms.is_empty() {
        root.extensions_used.push("KHR_texture_transform".to_string());
    }
}

fn pad_to_4(data: &mut Vec<u8>) {
    while !data.len().is_multiple_of(4) {
        data.push(0);
    }
}

fn bounding_coords(points: &[[f32; 3]]) -> ([f32; 3], [f32; 3]) {
    let mut min = [f32::MAX; 3];
    let mut max = [f32::MIN; 3];
    for p in points {
        for i in 0..3 {
            min[i] = f32::min(min[i], p[i]);
            max[i] = f32::max(max[i], p[i]);
        }
    }
    (min, max)
}

/// Per-triangle metadata for interactive armor viewers.
///
/// Each entry describes one triangle's collision material, zone classification,
/// armor thickness, and display color. Consumers can use this for hover/click
/// tooltips in a 3D viewer.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ArmorTriangleInfo {
    /// 1-based armor model index (matches GameParams key prefix).
    pub model_index: u32,
    /// 0-based triangle index within the armor model.
    pub triangle_index: u32,
    /// Collision material ID (0–255) from the BVH node header.
    pub material_id: u8,
    /// Human-readable material name (e.g. "Cit_Belt", "Bow_Bottom").
    pub material_name: String,
    /// Zone classification (e.g. "Citadel", "Bow", "Superstructure").
    pub zone: String,
    /// Total armor thickness in millimeters (sum of all layers).
    pub thickness_mm: f32,
    /// Per-layer thicknesses in mm, ordered by model_index.
    /// Single-layer materials have one entry; `Dual_*` materials have two or more.
    pub layers: Vec<f32>,
    /// RGBA color [0.0–1.0] encoding the total thickness via the game's color scale.
    pub color: [f32; 4],
    /// Whether the in-game armor viewer hides this plate. Plates in the "Hull"
    /// parent zone (generic materials like `Trans`, `Deck`, `Belt`) are never
    /// rendered by the viewer despite being functional in the combat model.
    pub hidden: bool,
}

/// Look up all non-zero armor layers for a material from mount armor (priority) or hull armor.
///
/// Returns `(layers, total)` where `layers` contains all non-zero thickness values
/// across model_indices, and `total` is their sum. This is used when we want to show
/// the per-plate thickness for the outermost layer and include all layers in the tooltip.
fn lookup_all_layers(mat_id: u32, mount_armor: Option<&ArmorMap>, armor_map: Option<&ArmorMap>) -> (Vec<f32>, f32) {
    let layers_map = mount_armor.and_then(|m| m.get(&mat_id)).or_else(|| armor_map.and_then(|m| m.get(&mat_id)));
    let layers: Vec<f32> = layers_map.map(|m| m.values().copied().filter(|&v| v > 0.0).collect()).unwrap_or_default();
    let total: f32 = layers.iter().sum();
    (layers, total)
}

/// Look up the armor thickness for a specific (material_id, model_index) pair.
/// Checks mount armor first, then hull armor as fallback.
fn lookup_thickness(
    mat_id: u32,
    model_index: u32,
    mount_armor: Option<&ArmorMap>,
    armor_map: Option<&ArmorMap>,
) -> f32 {
    mount_armor
        .and_then(|m| m.get(&mat_id))
        .and_then(|layers| layers.get(&model_index))
        .copied()
        .or_else(|| armor_map.and_then(|m| m.get(&mat_id)).and_then(|layers| layers.get(&model_index)).copied())
        .unwrap_or(0.0)
}

/// An indexed armor mesh with per-triangle metadata for interactive viewers.
///
/// Unlike `ArmorSubModel` (which groups by zone and loses per-triangle material info),
/// this type preserves full metadata for every triangle. Consumers can render the mesh
/// with `positions`/`normals`/`indices`/`colors`, then look up `triangle_info[face_index]`
/// on hover/click to display material name, thickness, and zone.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct InteractiveArmorMesh {
    /// Armor model name (e.g. "CM_PA_united").
    pub name: String,
    /// Vertex positions (3 per triangle, triangle-soup layout).
    pub positions: Vec<[f32; 3]>,
    /// Vertex normals (same length as positions).
    pub normals: Vec<[f32; 3]>,
    /// Triangle indices into positions/normals (length = triangle_count * 3).
    pub indices: Vec<u32>,
    /// Per-vertex RGBA color encoding armor thickness.
    /// All 3 vertices of a triangle share the same color.
    pub colors: Vec<[f32; 4]>,
    /// Per-triangle metadata. `triangle_info[i]` corresponds to
    /// `indices[i*3..i*3+3]`. Length = `indices.len() / 3`.
    pub triangle_info: Vec<ArmorTriangleInfo>,
    /// Optional world-space transform (column-major 4x4).
    /// Used for turret armor instances positioned at mount points.
    /// Hull armor meshes have `None` (already in world space).
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub transform: Option<[f32; 16]>,
}

impl InteractiveArmorMesh {
    /// Build an `InteractiveArmorMesh` from a parsed `ArmorModel`.
    ///
    /// `armor_map` is the hull-wide [`ArmorMap`] (`A_Hull.armor`).
    /// `mount_armor` is the optional per-mount [`ArmorMap`] (`A_Artillery.HP_XXX.armor`).
    /// Mount armor is checked first, then hull armor as fallback.
    pub fn from_armor_model(
        armor: &crate::models::geometry::ArmorModel,
        armor_map: Option<&ArmorMap>,
        mount_armor: Option<&ArmorMap>,
    ) -> Self {
        let tri_count = armor.triangles.len();
        let vert_count = tri_count * 3;
        let mut positions = Vec::with_capacity(vert_count);
        let mut normals = Vec::with_capacity(vert_count);
        let mut indices = Vec::with_capacity(vert_count);
        let mut colors = Vec::with_capacity(vert_count);
        let mut triangle_info = Vec::with_capacity(tri_count);

        for (ti, tri) in armor.triangles.iter().enumerate() {
            let mat_name = collision_material_name(tri.material_id);
            let zone = zone_from_material_name(mat_name).to_string();

            let mat_id = tri.material_id as u32;
            let layer = tri.layer_index as u32;
            // Use the per-triangle layer_index for the specific plate thickness.
            let thickness_mm = lookup_thickness(mat_id, layer, mount_armor, armor_map);
            // Collect all non-zero layers for the tooltip (shows stacked plates).
            let (all_layers, _) = lookup_all_layers(mat_id, mount_armor, armor_map);
            let color = thickness_to_color(thickness_mm);

            // Negate Z for left→right-handed conversion.
            for v in 0..3 {
                let [px, py, pz] = tri.vertices[v];
                positions.push([px, py, -pz]);
                let [nx, ny, nz] = tri.normals[v];
                normals.push([nx, ny, -nz]);
                indices.push((ti * 3 + v) as u32);
                colors.push(color);
            }

            let hidden = matches!(zone.as_str(), "Hull" | "SteeringGear" | "Default");
            triangle_info.push(ArmorTriangleInfo {
                model_index: layer,
                triangle_index: ti as u32,
                material_id: tri.material_id,
                material_name: mat_name.to_string(),
                zone,
                thickness_mm,
                layers: if all_layers.len() > 1 { all_layers } else { vec![thickness_mm] },
                color,
                hidden,
            });
        }

        Self { name: armor.name.clone(), positions, normals, indices, colors, triangle_info, transform: None }
    }
}

/// An armor mesh ready for glTF export (triangle soup, no textures).
pub struct ArmorSubModel {
    pub name: String,
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
    /// Per-vertex RGBA color encoding armor thickness.
    /// All 3 vertices of a triangle share the same color.
    pub colors: Vec<[f32; 4]>,
    /// Optional world-space transform (column-major 4x4).
    /// Used for turret armor instances positioned at mount points.
    pub transform: Option<[f32; 16]>,
}

impl ArmorSubModel {
    /// Build an `ArmorSubModel` from a parsed `ArmorModel`.
    ///
    /// See [`InteractiveArmorMesh::from_armor_model`] for parameter docs.
    pub fn from_armor_model(
        armor: &crate::models::geometry::ArmorModel,
        armor_map: Option<&ArmorMap>,
        mount_armor: Option<&ArmorMap>,
    ) -> Self {
        let tri_count = armor.triangles.len();
        let vert_count = tri_count * 3;
        let mut positions = Vec::with_capacity(vert_count);
        let mut normals = Vec::with_capacity(vert_count);
        let mut indices = Vec::with_capacity(vert_count);
        let mut colors = Vec::with_capacity(vert_count);

        for (ti, tri) in armor.triangles.iter().enumerate() {
            let mat_id = tri.material_id as u32;
            let layer = tri.layer_index as u32;
            let thickness_mm = lookup_thickness(mat_id, layer, mount_armor, armor_map);
            let color = thickness_to_color(thickness_mm);

            // Negate Z for left→right-handed conversion.
            for v in 0..3 {
                let [px, py, pz] = tri.vertices[v];
                positions.push([px, py, -pz]);
                let [nx, ny, nz] = tri.normals[v];
                normals.push([nx, ny, -nz]);
                indices.push((ti * 3 + v) as u32);
                colors.push(color);
            }
        }

        Self { name: armor.name.clone(), positions, normals, indices, colors, transform: None }
    }
}

/// A hull visual mesh for interactive viewers (positions, normals, indices + render set name).
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct InteractiveHullMesh {
    /// Render set name (e.g. "Hull", "Superstructure").
    pub name: String,
    /// Vertex positions.
    pub positions: Vec<[f32; 3]>,
    /// Vertex normals (same length as positions).
    pub normals: Vec<[f32; 3]>,
    /// Vertex UVs (same length as positions).
    pub uvs: Vec<[f32; 2]>,
    /// Triangle indices into positions/normals/uvs.
    pub indices: Vec<u32>,
    /// Full VFS path to the .mfm material file (for texture lookup).
    pub mfm_path: Option<String>,
    /// Baked per-vertex colors from albedo texture (same length as positions, or empty for fallback).
    pub colors: Vec<[f32; 4]>,
    /// Optional world-space transform (column-major 4x4) for turret mounts.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub transform: Option<[f32; 16]>,
}

/// Collect hull visual meshes (render sets) from a visual prototype and geometry.
///
/// Each render set becomes one `InteractiveHullMesh` with decoded
/// positions, normals, UVs, and indices. The caller is responsible for
/// baking textures into vertex colors using the `mfm_path` field.
pub fn collect_hull_meshes(
    visual: &VisualPrototype,
    geometry: &MergedGeometry,
    db: &PrototypeDatabase<'_>,
    lod: usize,
    damaged: bool,
    barrel_pitch: Option<&BarrelPitch>,
) -> Result<Vec<InteractiveHullMesh>, Report<ExportError>> {
    let mut result = Vec::new();

    if visual.lods.is_empty() || lod >= visual.lods.len() {
        return Ok(result);
    }
    let lod_entry = &visual.lods[lod];

    let exclude = if damaged { DAMAGED_EXCLUDE } else { INTACT_EXCLUDE };

    let self_id_index = db.build_self_id_index();

    for &rs_name_id in &lod_entry.render_set_names {
        let rs = visual
            .render_sets
            .iter()
            .find(|rs| rs.name_id == rs_name_id)
            .ok_or_else(|| Report::new(ExportError::RenderSetNotFound(rs_name_id)))?;

        let rs_name = db.strings.get_string_by_id(rs_name_id).unwrap_or("<unknown>");

        if exclude.iter().any(|sub| rs_name.contains(sub)) {
            continue;
        }

        let vertices_mapping_id = rs.vertices_mapping_id;
        let indices_mapping_id = rs.indices_mapping_id;

        let vert_mapping = geometry
            .vertices_mapping
            .iter()
            .find(|m| m.mapping_id == vertices_mapping_id)
            .ok_or_else(|| Report::new(ExportError::VerticesMappingNotFound { id: vertices_mapping_id }))?;

        let idx_mapping = geometry
            .indices_mapping
            .iter()
            .find(|m| m.mapping_id == indices_mapping_id)
            .ok_or_else(|| Report::new(ExportError::IndicesMappingNotFound { id: indices_mapping_id }))?;

        let vbuf_idx = vert_mapping.merged_buffer_index as usize;
        if vbuf_idx >= geometry.merged_vertices.len() {
            return Err(Report::new(ExportError::BufferIndexOutOfRange {
                index: vbuf_idx,
                count: geometry.merged_vertices.len(),
            }));
        }
        let vert_proto = &geometry.merged_vertices[vbuf_idx];

        let ibuf_idx = idx_mapping.merged_buffer_index as usize;
        if ibuf_idx >= geometry.merged_indices.len() {
            return Err(Report::new(ExportError::BufferIndexOutOfRange {
                index: ibuf_idx,
                count: geometry.merged_indices.len(),
            }));
        }
        let idx_proto = &geometry.merged_indices[ibuf_idx];

        let decoded_vertices =
            vert_proto.data.decode().map_err(|e| Report::new(ExportError::VertexDecode(format!("{e:?}"))))?;
        let decoded_indices =
            idx_proto.data.decode().map_err(|e| Report::new(ExportError::IndexDecode(format!("{e:?}"))))?;

        let format = vertex_format::parse_vertex_format(&vert_proto.format_name);
        let stride = vert_proto.stride_in_bytes as usize;

        let vert_offset = vert_mapping.items_offset as usize;
        let vert_count = vert_mapping.items_count as usize;
        let vert_start = vert_offset * stride;
        let vert_end = vert_start + vert_count * stride;

        if vert_end > decoded_vertices.len() {
            return Err(Report::new(ExportError::VertexDecode(format!(
                "vertex range {}..{} exceeds buffer size {}",
                vert_start,
                vert_end,
                decoded_vertices.len()
            ))));
        }
        let vert_slice = &decoded_vertices[vert_start..vert_end];

        let idx_offset = idx_mapping.items_offset as usize;
        let idx_count = idx_mapping.items_count as usize;
        let index_size = idx_proto.index_size as usize;
        let idx_start = idx_offset * index_size;
        let idx_end = idx_start + idx_count * index_size;

        if idx_end > decoded_indices.len() {
            return Err(Report::new(ExportError::IndexDecode(format!(
                "index range {}..{} exceeds buffer size {}",
                idx_start,
                idx_end,
                decoded_indices.len()
            ))));
        }
        let idx_slice = &decoded_indices[idx_start..idx_end];

        let indices: Vec<u32> = match index_size {
            2 => idx_slice.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]]) as u32).collect(),
            4 => idx_slice.chunks_exact(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect(),
            _ => {
                return Err(Report::new(ExportError::IndexDecode(format!("unsupported index size: {index_size}"))));
            }
        };

        let mut verts = unpack_vertices(vert_slice, stride, &format);

        // Apply per-vertex barrel pitch rotation if configured.
        if let Some(bp) = barrel_pitch {
            apply_barrel_pitch(&mut verts.positions, &mut verts.normals, vert_slice, stride, &format, bp);
        }

        // Resolve full MFM path for texture lookup.
        let mfm_path = if rs.material_mfm_path_id != 0 {
            self_id_index.get(&rs.material_mfm_path_id).map(|&idx| db.reconstruct_path(idx, &self_id_index))
        } else {
            None
        };

        result.push(InteractiveHullMesh {
            name: rs_name.to_string(),
            positions: verts.positions,
            normals: verts.normals,
            uvs: verts.uvs,
            indices,
            mfm_path,
            colors: Vec::new(),
            transform: None,
        });
    }

    Ok(result)
}

/// Game's exact armor thickness color scale.
///
/// 10 color buckets matching the in-game visualization from `ArmorConstants.py`.
/// Each entry: (max_thickness_mm, r, g, b).
/// Assignment uses `bisect_left` — a thickness of exactly a breakpoint value
/// falls into that breakpoint's bucket.
const ARMOR_COLOR_SCALE: &[(f32, f32, f32, f32)] = &[
    (14.0, 110.0 / 255.0, 209.0 / 255.0, 176.0 / 255.0), // teal
    (16.0, 149.0 / 255.0, 210.0 / 255.0, 127.0 / 255.0), // light green
    (24.0, 170.0 / 255.0, 201.0 / 255.0, 102.0 / 255.0), // yellow-green
    (26.0, 192.0 / 255.0, 193.0 / 255.0, 80.0 / 255.0),  // olive
    (28.0, 226.0 / 255.0, 195.0 / 255.0, 62.0 / 255.0),  // gold
    (33.0, 225.0 / 255.0, 171.0 / 255.0, 54.0 / 255.0),  // orange-gold
    (75.0, 227.0 / 255.0, 144.0 / 255.0, 49.0 / 255.0),  // orange
    (160.0, 230.0 / 255.0, 115.0 / 255.0, 49.0 / 255.0), // dark orange
    (399.0, 220.0 / 255.0, 78.0 / 255.0, 48.0 / 255.0),  // red-orange
    (999.0, 185.0 / 255.0, 47.0 / 255.0, 48.0 / 255.0),  // dark red
];

/// Map armor thickness (mm) to an RGBA color matching the game's visualization.
///
/// Uses the exact 10-bucket color scale from the game's `ArmorConstants.py`.
/// Thickness ≤ 0 is treated as unknown (faint blue).
pub fn thickness_to_color(thickness_mm: f32) -> [f32; 4] {
    if thickness_mm <= 0.0 {
        return [0.8, 0.8, 0.8, 0.5]; // light gray for plates with no assigned thickness
    }

    // bisect_left: find first bucket where breakpoint >= thickness
    let idx =
        ARMOR_COLOR_SCALE.iter().position(|&(bp, _, _, _)| thickness_mm <= bp).unwrap_or(ARMOR_COLOR_SCALE.len() - 1);

    let (_, r, g, b) = ARMOR_COLOR_SCALE[idx];
    [r, g, b, 0.8]
}

/// An entry in the armor thickness color legend.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ArmorLegendEntry {
    /// Lower bound of this thickness range (mm), inclusive.
    pub min_mm: f32,
    /// Upper bound of this thickness range (mm), inclusive.
    pub max_mm: f32,
    /// RGBA color used in the GLB export, each component 0.0..1.0.
    pub color: [f32; 4],
    /// Human-readable color name.
    pub color_name: String,
}

/// Return the armor thickness color legend.
///
/// Each entry describes one color bucket: the thickness range (mm) and the
/// color used. External tools can use this to build UI legends, filter by
/// exact mm ranges, or map thickness values to colors programmatically.
pub fn armor_color_legend() -> Vec<ArmorLegendEntry> {
    let color_names = [
        "teal",
        "light green",
        "yellow-green",
        "olive",
        "gold",
        "orange-gold",
        "orange",
        "dark orange",
        "red-orange",
        "dark red",
    ];

    ARMOR_COLOR_SCALE
        .iter()
        .enumerate()
        .map(|(i, &(max_mm, r, g, b))| {
            let min_mm = if i == 0 { 0.0 } else { ARMOR_COLOR_SCALE[i - 1].0 + 1.0 };
            ArmorLegendEntry { min_mm, max_mm, color: [r, g, b, 0.8], color_name: color_names[i].to_string() }
        })
        .collect()
}

/// Derive the zone name from a collision material name.
///
/// Material names follow patterns like `Bow_Bottom`, `Cit_Belt`, `SS_Side`,
/// `Tur1GkBar`, `RudderAft`, etc. The prefix before the first `_` determines
/// the zone, with special handling for turret and rudder names.
pub fn zone_from_material_name(mat_name: &str) -> &'static str {
    use std::collections::HashSet;
    use std::sync::Mutex;
    static WARNED: Mutex<Option<HashSet<String>>> = Mutex::new(None);

    // Dual-zone materials: Dual_<primary>_<secondary>_<part>.
    // Use the first zone identifier after "Dual_" as the primary.
    if let Some(rest) = mat_name.strip_prefix("Dual_") {
        if rest.starts_with("Cit") {
            return "Citadel";
        }
        if rest.starts_with("OCit") {
            return "Citadel";
        }
        if rest.starts_with("Cas") {
            return "Casemate";
        }
        if rest.starts_with("SSC") {
            return "Superstructure";
        }
        if rest.starts_with("Bow") {
            return "Bow";
        }
        if rest.starts_with("St_") {
            return "Stern";
        }
        if rest.starts_with("SS_") {
            return "Superstructure";
        }
        {
            let mut warned = WARNED.lock().unwrap();
            let set = warned.get_or_insert_with(HashSet::new);
            if set.insert(mat_name.to_string()) {
                eprintln!(
                    "BUG: unrecognized Dual_ collision material '{mat_name}' — \
                     zone_from_material_name needs updating"
                );
            }
        }
        return "Other";
    }
    // Zone sub-face materials: Side/Deck/Trans/Inclin + zone suffix.
    if mat_name.ends_with("Cit") {
        return "Citadel";
    }
    if mat_name.ends_with("Cas") {
        return "Casemate";
    }
    if mat_name.ends_with("SSC") {
        return "Superstructure";
    }
    if mat_name.ends_with("Bow") {
        return "Bow";
    }
    if mat_name.ends_with("Stern") {
        return "Stern";
    }
    if mat_name.ends_with("SS") && !mat_name.starts_with("Dual_") {
        // SGBarbetteSS, SGDownSS → SteeringGear; DeckSS, SideSS, TransSS → Superstructure
        if mat_name.starts_with("SG") {
            return "SteeringGear";
        }
        return "Superstructure";
    }
    // Collision material prefixes.
    if mat_name.starts_with("Bow") {
        return "Bow";
    }
    if mat_name.starts_with("St_") {
        return "Stern";
    }
    if mat_name.starts_with("Cit") {
        return "Citadel";
    }
    if mat_name.starts_with("OCit") {
        return "Citadel";
    }
    if mat_name.starts_with("Cas") {
        return "Casemate";
    }
    if mat_name.starts_with("SSC") || mat_name == "SSCasemate" {
        return "Superstructure";
    }
    if mat_name.starts_with("SS_") {
        return "Superstructure";
    }
    if mat_name.starts_with("Tur") || mat_name.starts_with("AuTurret") {
        return "Turret";
    }
    if mat_name.starts_with("Art") {
        return "Turret";
    }
    if mat_name.starts_with("Rudder") || mat_name.starts_with("SG") {
        return "SteeringGear";
    }
    if mat_name.starts_with("Bulge") {
        return "TorpedoProtection";
    }
    if mat_name.starts_with("Bridge") || mat_name.starts_with("Funnel") {
        return "Superstructure";
    }
    if mat_name.starts_with("Kdp") {
        return "Hull";
    }
    match mat_name {
        "Deck" | "ConstrSide" | "Hull" | "Side" | "Bottom" | "Top" | "Belt" | "Trans" | "Inclin" => "Hull",
        "common" | "zero" => "Default",
        _ => {
            let mut warned = WARNED.lock().unwrap();
            let set = warned.get_or_insert_with(HashSet::new);
            if set.insert(mat_name.to_string()) {
                eprintln!(
                    "BUG: unrecognized collision material '{mat_name}' — \
                     zone_from_material_name needs updating"
                );
            }
            "Other"
        }
    }
}

/// The built-in collision material name table.
///
/// Contiguous array indexed by material ID (0..=250). Extracted from the game
/// client's `py_collisionMaterialName` table at 0x142a569a0.
const COLLISION_MATERIAL_NAMES: &[&str] = &[
    // 0-1: generic
    "common", // 0
    "zero",   // 1
    // 2-31: Dual-zone materials
    "Dual_SSC_Bow_Side",       // 2
    "Dual_SSC_St_Side",        // 3
    "Dual_Cas_OCit_Belt",      // 4
    "Dual_OCit_St_Trans",      // 5
    "Dual_OCit_Bow_Trans",     // 6
    "Dual_Cit_Bow_Side",       // 7
    "Dual_Cit_Bow_Belt",       // 8
    "Dual_Cit_Bow_ArtSide",    // 9
    "Dual_Cit_St_Side",        // 10
    "Dual_Cit_St_Belt",        // 11
    "Bottom",                  // 12
    "Dual_Cit_St_ArtSide",     // 13
    "Dual_Cas_Bow_Belt",       // 14
    "Dual_Cas_St_Belt",        // 15
    "Dual_Cas_SSC_Belt",       // 16
    "Dual_SSC_Bow_ConstrSide", // 17
    "Dual_SSC_St_ConstrSide",  // 18
    "Cas_Inclin",              // 19
    "SSC_Inclin",              // 20
    "Dual_Cas_SSC_Inclin",     // 21
    "Dual_Cas_Bow_Inclin",     // 22
    "Dual_Cas_St_Inclin",      // 23
    "Dual_SSC_Bow_Inclin",     // 24
    "Dual_SSC_St_Inclin",      // 25
    "Dual_Cit_Bow_Bulge",      // 26
    "Dual_Cit_St_Bulge",       // 27
    "Dual_Cas_SS_Belt",        // 28
    "Dual_Cit_Cas_ArtDeck",    // 29
    "Dual_Cit_Cas_ArtSide",    // 30
    "Dual_OCit_OCit_Side",     // 31
    // 32-45: turret/artillery/auxiliary turret
    "TurretSide",       // 32
    "TurretTop",        // 33
    "TurretFront",      // 34
    "TurretAft",        // 35
    "FunnelSide",       // 36
    "ArtBottom",        // 37
    "ArtSide",          // 38
    "ArtTop",           // 39
    "AuTurretAft",      // 40
    "AuTurretBarbette", // 41
    "AuTurretDown",     // 42
    "AuTurretFwd",      // 43
    "AuTurretSide",     // 44
    "AuTurretTop",      // 45
    // 46-51: Bow
    "Bow_Belt",       // 46
    "Bow_Bottom",     // 47
    "Bow_ConstrSide", // 48
    "Bow_Deck",       // 49
    "Bow_Inclin",     // 50
    "Bow_Trans",      // 51
    // 52-54: Bridge
    "BridgeBottom", // 52
    "BridgeSide",   // 53
    "BridgeTop",    // 54
    // 55-58: Casemate
    "Cas_AftTrans", // 55
    "Cas_Belt",     // 56
    "Cas_Deck",     // 57
    "Cas_FwdTrans", // 58
    // 59-68: Citadel
    "Cit_AftTrans",       // 59
    "Cit_Barbette",       // 60
    "Cit_Belt",           // 61
    "Cit_Bottom",         // 62
    "Cit_Bulge",          // 63
    "Cit_Deck",           // 64
    "Cit_FwdTrans",       // 65
    "Cit_Inclin",         // 66
    "Cit_Side",           // 67
    "Dual_Cit_Cas_Bulge", // 68
    // 69-79: Hull/misc
    "ConstrSide",        // 69
    "Dual_Cit_Cas_Belt", // 70
    "Bow_Fdck",          // 71
    "St_Fdck",           // 72
    "KdpBottom",         // 73
    "KdpSide",           // 74
    "KdpTop",            // 75
    "OCit_AftTrans",     // 76
    "OCit_Belt",         // 77
    "OCit_Deck",         // 78
    "OCit_FwdTrans",     // 79
    // 80-83: Rudder
    "RudderAft",  // 80
    "RudderFwd",  // 81
    "RudderSide", // 82
    "RudderTop",  // 83
    // 84-90: Superstructure casemate / Superstructure
    "SSC_AftTrans",   // 84
    "SSCasemate",     // 85
    "SSC_ConstrSide", // 86
    "SSC_Deck",       // 87
    "SSC_FwdTrans",   // 88
    "SS_Side",        // 89
    "SS_Top",         // 90
    // 91-96: Stern
    "St_Belt",       // 91
    "St_Bottom",     // 92
    "St_ConstrSide", // 93
    "St_Deck",       // 94
    "St_Inclin",     // 95
    "St_Trans",      // 96
    // 97-106: Turret generic / hull generic
    "TurretBarbette",     // 97
    "TurretBarbette2",    // 98
    "TurretDown",         // 99
    "TurretFwd",          // 100
    "Bulge",              // 101
    "Trans",              // 102
    "Deck",               // 103
    "Belt",               // 104
    "Dual_Cit_SSC_Bulge", // 105
    "Inclin",             // 106
    // 107-110: SS/Bridge, Casemate bottom
    "SS_BridgeTop",    // 107
    "SS_BridgeSide",   // 108
    "SS_BridgeBottom", // 109
    "Cas_Bottom",      // 110
    // 111-133: Zone sub-face materials (Side/Deck/Trans/Inclin per zone)
    "SideCit",     // 111
    "DeckCit",     // 112
    "TransCit",    // 113
    "InclinCit",   // 114
    "SideCas",     // 115
    "DeckCas",     // 116
    "TransCas",    // 117
    "InclinCas",   // 118
    "SideSSC",     // 119
    "DeckSSC",     // 120
    "TransSSC",    // 121
    "InclinSSC",   // 122
    "SideBow",     // 123
    "DeckBow",     // 124
    "TransBow",    // 125
    "InclinBow",   // 126
    "SideStern",   // 127
    "DeckStern",   // 128
    "TransStern",  // 129
    "InclinStern", // 130
    "SideSS",      // 131
    "DeckSS",      // 132
    "TransSS",     // 133
    // 134-153: Turret barbettes (GkBar) for turrets 1-20
    "Tur1GkBar",  // 134
    "Tur2GkBar",  // 135
    "Tur3GkBar",  // 136
    "Tur4GkBar",  // 137
    "Tur5GkBar",  // 138
    "Tur6GkBar",  // 139
    "Tur7GkBar",  // 140
    "Tur8GkBar",  // 141
    "Tur9GkBar",  // 142
    "Tur10GkBar", // 143
    "Tur11GkBar", // 144
    "Tur12GkBar", // 145
    "Tur13GkBar", // 146
    "Tur14GkBar", // 147
    "Tur15GkBar", // 148
    "Tur16GkBar", // 149
    "Tur17GkBar", // 150
    "Tur18GkBar", // 151
    "Tur19GkBar", // 152
    "Tur20GkBar", // 153
    // 154-173: Dual-zone transitions (Cas/SSC/Bow/St/SS combinations)
    "Dual_Cas_Bow_Trans",  // 154
    "Dual_Cas_Bow_Deck",   // 155
    "Dual_Cas_St_Trans",   // 156
    "Dual_Cas_St_Deck",    // 157
    "Dual_Cas_SSC_Deck",   // 158
    "Dual_Cas_SSC_Trans",  // 159
    "Dual_Cas_SS_Deck",    // 160
    "Dual_Cas_SS_Trans",   // 161
    "Dual_SSC_Bow_Trans",  // 162
    "Dual_SSC_Bow_Deck",   // 163
    "Dual_SSC_St_Trans",   // 164
    "Dual_SSC_St_Deck",    // 165
    "Dual_SSC_SS_Deck",    // 166
    "Dual_SSC_SS_Trans",   // 167
    "Dual_Bow_SS_Deck",    // 168
    "Dual_Bow_SS_Trans",   // 169
    "Dual_St_SS_Deck",     // 170
    "Dual_St_SS_Trans",    // 171
    "Dual_Cit_Bow_Bottom", // 172
    "Dual_Cit_St_Bottom",  // 173
    // 174-193: Turret undersides (GkDown) for turrets 1-20
    "Tur1GkDown",  // 174
    "Tur2GkDown",  // 175
    "Tur3GkDown",  // 176
    "Tur4GkDown",  // 177
    "Tur5GkDown",  // 178
    "Tur6GkDown",  // 179
    "Tur7GkDown",  // 180
    "Tur8GkDown",  // 181
    "Tur9GkDown",  // 182
    "Tur10GkDown", // 183
    "Tur11GkDown", // 184
    "Tur12GkDown", // 185
    "Tur13GkDown", // 186
    "Tur14GkDown", // 187
    "Tur15GkDown", // 188
    "Tur16GkDown", // 189
    "Tur17GkDown", // 190
    "Tur18GkDown", // 191
    "Tur19GkDown", // 192
    "Tur20GkDown", // 193
    // 194-213: Dual same-zone / cross-zone combinations
    "Dual_Cit_Cit_Deck",       // 194
    "Dual_Cit_Cit_Inclin",     // 195
    "Dual_Cit_Cit_Trans",      // 196
    "Dual_Cit_Cit_Side",       // 197
    "Dual_Cas_Cas_Belt",       // 198
    "Dual_Cas_Cas_Deck",       // 199
    "Dual_SSC_SSC_ConstrSide", // 200
    "Dual_SSC_SSC_Deck",       // 201
    "Dual_Bow_Bow_Deck",       // 202
    "Dual_Bow_Bow_ConstrSide", // 203
    "Dual_St_St_Deck",         // 204
    "Dual_St_St_ConstrSide",   // 205
    "Dual_SS_SS_Top",          // 206
    "Dual_SS_SS_Side",         // 207
    "Dual_Cit_Bow_ArtDeck",    // 208
    "Dual_Cit_St_ArtDeck",     // 209
    "Dual_Cas_Bow_Side",       // 210
    "Dual_Cas_St_Side",        // 211
    "Dual_Cit_Cas_Side",       // 212
    "Dual_Cit_SSC_Side",       // 213
    // 214-233: Turret tops (GkTop) for turrets 1-20
    "Tur1GkTop",  // 214
    "Tur2GkTop",  // 215
    "Tur3GkTop",  // 216
    "Tur4GkTop",  // 217
    "Tur5GkTop",  // 218
    "Tur6GkTop",  // 219
    "Tur7GkTop",  // 220
    "Tur8GkTop",  // 221
    "Tur9GkTop",  // 222
    "Tur10GkTop", // 223
    "Tur11GkTop", // 224
    "Tur12GkTop", // 225
    "Tur13GkTop", // 226
    "Tur14GkTop", // 227
    "Tur15GkTop", // 228
    "Tur16GkTop", // 229
    "Tur17GkTop", // 230
    "Tur18GkTop", // 231
    "Tur19GkTop", // 232
    "Tur20GkTop", // 233
    // 234-241: Hangar/forecastle deck, steering gear barbette
    "Cas_Hang",      // 234
    "Cas_Fdck",      // 235
    "SSC_Fdck",      // 236
    "SSC_Hang",      // 237
    "SS_SGBarbette", // 238
    "SS_SGDown",     // 239
    "SGBarbetteSS",  // 240
    "SGDownSS",      // 241
    // 242-254: Dual Citadel zone transitions
    "Dual_Cit_Cas_Deck",   // 242
    "Dual_Cit_Cas_Inclin", // 243
    "Dual_Cit_Cas_Trans",  // 244
    "Dual_Cit_SSC_Deck",   // 245
    "Dual_Cit_SSC_Inclin", // 246
    "Dual_Cit_SSC_Trans",  // 247
    "Dual_Cit_Bow_Trans",  // 248
    "Dual_Cit_Bow_Inclin", // 249
    "Dual_Cit_Bow_Deck",   // 250
    "Dual_Cit_St_Trans",   // 251
    "Dual_Cit_St_Inclin",  // 252
    "Dual_Cit_St_Deck",    // 253
    "Dual_Cit_SS_Deck",    // 254
];

/// Look up the collision material name for a given material ID.
///
/// Logs a warning for unknown IDs — this indicates the game's material table
/// has been extended and our hardcoded copy needs updating.
pub fn collision_material_name(id: u8) -> &'static str {
    use std::sync::Mutex;
    static WARNED: Mutex<[bool; 256]> = Mutex::new([false; 256]);

    let idx = id as usize;
    if idx < COLLISION_MATERIAL_NAMES.len() {
        COLLISION_MATERIAL_NAMES[idx]
    } else {
        let mut warned = WARNED.lock().unwrap();
        if !warned[idx] {
            warned[idx] = true;
            eprintln!(
                "BUG: collision material ID {id} is beyond the known table (max {}). \
                 The game's material table has likely been updated — \
                 see MODELS.md for how to re-extract it.",
                COLLISION_MATERIAL_NAMES.len() - 1
            );
        }
        "unknown"
    }
}

/// Split an armor model into per-zone `ArmorSubModel`s for selective visibility in Blender.
///
/// Each triangle is classified by its collision material ID, which determines both:
/// - The armor thickness (looked up from the GameParams armor dict)
/// - The zone name (derived from the material name pattern)
///
/// Triangles are grouped into one mesh per zone for easy toggling in Blender.
pub fn armor_sub_models_by_zone(
    armor: &crate::models::geometry::ArmorModel,
    armor_map: Option<&ArmorMap>,
    mount_armor: Option<&ArmorMap>,
) -> Vec<ArmorSubModel> {
    // Group triangles by zone name.
    let mut zone_tris: std::collections::BTreeMap<String, Vec<(&crate::models::geometry::ArmorTriangle, [f32; 4])>> =
        std::collections::BTreeMap::new();

    for tri in &armor.triangles {
        let mat_id = tri.material_id as u32;
        let layer = tri.layer_index as u32;
        let thickness_mm = lookup_thickness(mat_id, layer, mount_armor, armor_map);
        let color = thickness_to_color(thickness_mm);

        let mat_name = collision_material_name(tri.material_id);
        let zone_name = zone_from_material_name(mat_name).to_string();

        zone_tris.entry(zone_name).or_default().push((tri, color));
    }

    // Build one ArmorSubModel per zone.
    zone_tris
        .into_iter()
        .map(|(zone_name, tris)| {
            let vert_count = tris.len() * 3;
            let mut positions = Vec::with_capacity(vert_count);
            let mut normals = Vec::with_capacity(vert_count);
            let mut indices = Vec::with_capacity(vert_count);
            let mut colors = Vec::with_capacity(vert_count);

            for (vi, (tri, color)) in tris.iter().enumerate() {
                for v in 0..3 {
                    positions.push(tri.vertices[v]);
                    normals.push(tri.normals[v]);
                    indices.push((vi * 3 + v) as u32);
                    colors.push(*color);
                }
            }

            ArmorSubModel { name: format!("Armor_{}", zone_name), positions, normals, indices, colors, transform: None }
        })
        .collect()
}

/// A named sub-model for multi-model ship export.
pub struct SubModel<'a> {
    pub name: String,
    pub visual: &'a VisualPrototype,
    pub geometry: &'a MergedGeometry<'a>,
    /// Optional world-space transform (column-major 4x4 matrix).
    /// If `None`, the sub-model is placed at the origin.
    pub transform: Option<[f32; 16]>,
    /// Group name for Blender outliner hierarchy (e.g. "Hull", "Main Battery").
    pub group: &'static str,
    /// If set, apply a pitch rotation to vertices weighted to barrel bones.
    pub barrel_pitch: Option<BarrelPitch>,
}

/// Configuration for per-vertex barrel pitch rotation.
/// Vertices whose dominant bone index is in `barrel_bone_indices` get their
/// position and normal transformed by `pitch_matrix` (turret-local space).
#[derive(Clone)]
pub struct BarrelPitch {
    /// 4x4 column-major pitch rotation matrix (around Rotate_X pivot).
    pub pitch_matrix: [f32; 16],
    /// Bone indices (into the render set's blend bone list) that are barrel bones.
    pub barrel_bone_indices: Vec<u8>,
}

/// Export multiple sub-models as a single GLB with separate named meshes/nodes.
///
/// Each sub-model becomes a separate selectable object in Blender.
/// `texture_set` contains base albedo + camo variant PNGs for material textures.
/// `armor_models` are added as additional untextured semi-transparent meshes.
pub fn export_ship_glb(
    sub_models: &[SubModel<'_>],
    armor_models: &[ArmorSubModel],
    db: &PrototypeDatabase<'_>,
    lod: usize,
    texture_set: &TextureSet,
    damaged: bool,
    writer: &mut impl Write,
) -> Result<(), Report<ExportError>> {
    let mut root = json::Root {
        asset: json::Asset {
            version: "2.0".to_string(),
            generator: Some("wowsunpack".to_string()),
            ..Default::default()
        },
        ..Default::default()
    };

    let mut bin_data: Vec<u8> = Vec::new();
    let mut mat_cache = MaterialCache::new();

    // Collect mesh nodes grouped by category.
    let mut grouped_nodes: BTreeMap<&str, Vec<json::Index<json::Node>>> = BTreeMap::new();

    let self_id_index = db.build_self_id_index();

    for sub in sub_models {
        // Validate LOD — skip sub-models that don't have enough LODs.
        if sub.visual.lods.is_empty() || lod >= sub.visual.lods.len() {
            eprintln!(
                "Warning: sub-model '{}' has {} LODs, skipping (requested LOD {})",
                sub.name,
                sub.visual.lods.len(),
                lod
            );
            continue;
        }

        let lod_entry = &sub.visual.lods[lod];
        let primitives = collect_primitives(
            sub.visual,
            sub.geometry,
            Some(db),
            Some(&self_id_index),
            lod_entry,
            damaged,
            sub.barrel_pitch.as_ref(),
        )?;

        if primitives.is_empty() {
            eprintln!("Warning: sub-model '{}' has no primitives for LOD {lod}", sub.name);
            continue;
        }

        let mut gltf_primitives = Vec::new();
        for prim in &primitives {
            let gltf_prim = add_primitive_to_root(&mut root, &mut bin_data, prim, texture_set, &mut mat_cache)?;
            gltf_primitives.push(gltf_prim);
        }

        // Create a mesh named after the sub-model.
        let mesh = root.push(json::Mesh {
            primitives: gltf_primitives,
            weights: None,
            name: Some(sub.name.clone()),
            extensions: Default::default(),
            extras: Default::default(),
        });

        // Create a node named after the sub-model, referencing the mesh.
        let node = root.push(json::Node {
            mesh: Some(mesh),
            name: Some(sub.name.clone()),
            matrix: sub.transform.map(negate_z_transform),
            ..Default::default()
        });

        grouped_nodes.entry(sub.group).or_default().push(node);
    }

    // Add armor meshes grouped under "Armor".
    let mut armor_nodes: Vec<json::Index<json::Node>> = Vec::new();
    for armor in armor_models {
        if armor.positions.is_empty() {
            continue;
        }

        let gltf_prim = add_armor_primitive_to_root(&mut root, &mut bin_data, armor)?;

        let mesh = root.push(json::Mesh {
            primitives: vec![gltf_prim],
            weights: None,
            name: Some(armor.name.clone()),
            extensions: Default::default(),
            extras: Default::default(),
        });

        let node = root.push(json::Node {
            mesh: Some(mesh),
            name: Some(armor.name.clone()),
            matrix: armor.transform.map(negate_z_transform),
            ..Default::default()
        });

        armor_nodes.push(node);
    }
    if !armor_nodes.is_empty() {
        grouped_nodes.insert("Armor", armor_nodes);
    }

    // Build scene hierarchy: one parent node per group.
    let mut scene_nodes = Vec::new();
    for (group_name, children) in &grouped_nodes {
        let parent = root.push(json::Node {
            children: Some(children.clone()),
            name: Some(group_name.to_string()),
            ..Default::default()
        });
        scene_nodes.push(parent);
    }

    // Pad binary data to 4-byte alignment.
    while !bin_data.len().is_multiple_of(4) {
        bin_data.push(0);
    }

    // Set the buffer byte_length.
    if !bin_data.is_empty() {
        let buffer = root.push(json::Buffer {
            byte_length: USize64::from(bin_data.len()),
            uri: None,
            name: None,
            extensions: Default::default(),
            extras: Default::default(),
        });
        for bv in root.buffer_views.iter_mut() {
            bv.buffer = buffer;
        }
    }

    // Create scene with group parent nodes.
    let scene = root.push(json::Scene {
        nodes: scene_nodes,
        name: None,
        extensions: Default::default(),
        extras: Default::default(),
    });
    root.scene = Some(scene);

    // Add KHR_materials_variants root extension if we have camo schemes.
    add_variants_extension(&mut root, texture_set);

    // Serialize and write GLB.
    let json_string =
        json::serialize::to_string(&root).map_err(|e| Report::new(ExportError::Serialize(e.to_string())))?;

    let glb = gltf::binary::Glb {
        header: gltf::binary::Header { magic: *b"glTF", version: 2, length: 0 },
        json: Cow::Owned(json_string.into_bytes()),
        bin: if bin_data.is_empty() { None } else { Some(Cow::Owned(bin_data)) },
    };

    glb.to_writer(writer).map_err(|e| Report::new(ExportError::Io(e.to_string())))?;

    Ok(())
}
