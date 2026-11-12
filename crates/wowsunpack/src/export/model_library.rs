//! Loads any catalogued `.model` on its own: meshes per render set, the
//! textures their materials reference, hardpoints and raw armor; and writes
//! one as a GLB.
//!
//! Everything a load returns sits in the model's node space, right-handed
//! (Z negated) like [`InteractiveHullMesh`] positions, so meshes, hardpoints
//! and armor line up without further transforms.

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::HashSet;
use std::io::Read;
use std::io::Write;
use std::sync::Arc;

use rootcause::Report;
use rootcause::prelude::ResultExt;
use thiserror::Error;
use vfs::VfsPath;

use crate::game_params::provider::GameMetadataProvider;
use crate::game_params::types::ArmorMap;
use crate::models::assets_bin::PrototypeDatabase;
use crate::models::assets_bin::SharedPrototypeDatabase;
use crate::models::geometry;
use crate::models::geometry::ArmorModel;
use crate::models::material::MaterialPrototype;
use crate::models::model;
use crate::models::model::ModelPrototype;
use crate::models::skeleton_extender::SkeletonExtender;
use crate::models::visual;
use crate::models::visual::VisualPrototype;

use super::camo_composite::RgbaImageData;
use super::gltf_export;
use super::gltf_export::HardpointSet;
use super::gltf_export::InteractiveArmorMesh;
use super::gltf_export::InteractiveHullMesh;
use super::gltf_export::SubModel;
use super::gltf_export::TextureSet;
use super::gltf_export::negate_z_transform;
use super::hardpoint::Hardpoint;
use super::hardpoint::ModelSpaceHardpoints;
use super::hardpoint::mesh_to_node_space;
use super::hardpoint::model_extenders;
use super::model_catalog;
use super::model_catalog::ModelCatalog;
use super::model_catalog::ModelEntry;
use super::model_catalog::ModelPath;
use super::model_catalog::ModelPathIndex;
use super::part_group::PartGroup;
use super::ship::ExportContents;
use super::texture;
use super::texture::TextureLod;

/// Why a [`ModelLibrary`] could not be built.
#[derive(Debug, Error)]
pub enum ModelLibraryError {
    #[error("assets.bin could not be parsed")]
    AssetsBin,
}

/// Why a model could not be loaded.
#[derive(Debug, Error)]
pub enum ModelLoadError {
    #[error("{path} is not a model prototype in assets.bin")]
    UnknownModel { path: ModelPath },
    #[error("the model record of {path} could not be read")]
    ModelRecord { path: ModelPath },
    #[error("{path} names no visual")]
    MissingVisual { path: ModelPath },
    #[error("visual 0x{visual_id:016X} of {path} is not a readable visual record")]
    VisualRecord { path: ModelPath, visual_id: u64 },
    #[error("geometry 0x{geometry_id:016X} of {visual_path} is not in the paths store")]
    UnresolvedGeometry { visual_path: String, geometry_id: u64 },
    /// The install indexes the model but ships no geometry for it: the path
    /// is absent from the packages, or listed there with no data.
    #[error("geometry {path} is not in the installed packages")]
    GeometryAbsent { path: String },
    #[error("geometry {path} could not be read")]
    GeometryRead { path: String },
    #[error("geometry {path} could not be parsed")]
    GeometryParse { path: String },
    #[error("the render sets of {visual_path} could not be decoded")]
    Meshes { visual_path: String },
}

/// Which material textures a load decodes. Paths are resolved for every
/// referenced map regardless; decoding is what costs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextureDecode {
    None,
    #[default]
    Albedo,
    /// Albedo, normal and metal-gloss maps.
    AllMaps,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ModelLoadOptions {
    /// Mesh LOD (0 = most detailed), clamped to each part's deepest.
    pub lod: usize,
    /// Show the destroyed state (crack geometry) instead of the intact one.
    pub damaged: bool,
    pub texture_lod: TextureLod,
    pub textures: TextureDecode,
}

/// What [`ModelLibrary::export_glb`] writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelExportOptions {
    /// Mesh LOD (0 = most detailed), clamped to each part's deepest.
    pub lod: usize,
    /// Write the destroyed state (crack geometry) instead of the intact one.
    pub damaged: bool,
    pub texture_lod: TextureLod,
    /// Embed albedo textures.
    pub textures: bool,
    pub contents: ExportContents,
    /// Write one empty node per hardpoint.
    pub hardpoints: bool,
}

/// The GameParams armor maps that give a model's armor plates thickness.
/// Without either map every plate reads 0 mm.
#[derive(Debug, Clone, Copy, Default)]
pub struct ModelArmorMaps<'a> {
    /// The hull `armor` map, the fallback for plates the mount map lacks.
    pub hull: Option<&'a ArmorMap>,
    /// A mount's own `armor` map, checked first.
    pub mount: Option<&'a ArmorMap>,
}

/// Why a model could not be exported.
#[derive(Debug, Error)]
pub enum ModelExportError {
    #[error("{path} could not be read for export")]
    Load { path: ModelPath },
    #[error("geometry {path} could not be parsed")]
    GeometryParse { path: String },
    #[error("the render sets of {visual_path} could not be read")]
    RenderSets { visual_path: String },
    #[error("the albedo of {mfm_path} could not be encoded")]
    TextureEncode { mfm_path: String },
    #[error("the GLB of {path} could not be written")]
    Write { path: ModelPath },
}

/// The shared, once-built index every model load and lookup goes through.
pub struct ModelLibrary {
    vfs: VfsPath,
    assets: SharedPrototypeDatabase,
    self_id_index: HashMap<u64, usize>,
    model_index: ModelPathIndex,
    /// Model by file stem, the first in store order winning, as the ship
    /// loader resolves misc models by name.
    models_by_stem: HashMap<String, ModelPath>,
    catalog: ModelCatalog,
}

impl ModelLibrary {
    /// Parses `assets.bin` once and keeps it parsed, indexes every `.model`,
    /// then groups the gameplay models with the skin links of `metadata`.
    pub fn new(
        vfs: VfsPath,
        assets_bin_bytes: Arc<Vec<u8>>,
        metadata: &GameMetadataProvider,
    ) -> Result<Self, Report<ModelLibraryError>> {
        let assets = SharedPrototypeDatabase::parse(assets_bin_bytes).context(ModelLibraryError::AssetsBin)?;
        let db = assets.db();
        let self_id_index = db.build_self_id_index();
        let model_index = ModelPathIndex::build(&db, &self_id_index);
        let catalog =
            ModelCatalog::from_paths(model_index.paths().cloned(), &model_catalog::peculiarity_models(metadata));

        let mut first_by_stem: HashMap<&str, (usize, &ModelPath)> = HashMap::new();
        for path in model_index.paths() {
            let Some(position) = model_index.position(path) else {
                continue;
            };
            let slot = first_by_stem.entry(path.stem()).or_insert((position, path));
            if position < slot.0 {
                *slot = (position, path);
            }
        }
        let models_by_stem =
            first_by_stem.into_iter().map(|(stem, (_, path))| (stem.to_string(), path.clone())).collect();

        Ok(Self { vfs, assets, self_id_index, model_index, models_by_stem, catalog })
    }

    pub(crate) fn db(&self) -> PrototypeDatabase<'_> {
        self.assets.db()
    }

    pub(crate) fn self_id_index(&self) -> &HashMap<u64, usize> {
        &self.self_id_index
    }

    /// The model named `stem`, as the ship loader resolves misc models by name.
    pub(crate) fn model_by_stem(&self, stem: &str) -> Option<&ModelPath> {
        self.models_by_stem.get(stem)
    }

    /// The skeleton extenders of the model at `path`, empty when it has none
    /// or its record does not read.
    pub(crate) fn model_extenders(&self, db: &PrototypeDatabase<'_>, path: &ModelPath) -> Vec<SkeletonExtender> {
        match self.read_model_prototype(db, path) {
            Ok(model) => model_extenders(db, &model),
            Err(_) => Vec::new(),
        }
    }

    pub fn catalog(&self) -> &ModelCatalog {
        &self.catalog
    }

    pub fn vfs(&self) -> &VfsPath {
        &self.vfs
    }

    /// The LOD count and armor presence of `path` and its sections, read
    /// without building meshes or textures.
    pub fn model_summary(&self, path: &ModelPath) -> Result<ModelSummary, Report<ModelLoadError>> {
        let db = &self.assets.db();
        let sources = self.read_parts(db, path)?;
        let mut summary = ModelSummary { lod_count: 0, has_armor: false };
        for source in &sources {
            summary.lod_count = summary.lod_count.max(source.visual.lods.len());
            if !summary.has_armor {
                let armor_models = geometry::armor_model_count(&source.geometry_bytes)
                    .context_with(|| ModelLoadError::GeometryParse { path: source.geometry_path.clone() })?;
                summary.has_armor = armor_models > 0;
            }
        }

        Ok(summary)
    }

    /// Loads `path` with every section the catalogue lists for it: a ship's
    /// own visual holds only a distant silhouette, its sections the hull.
    pub fn load_model(
        &self,
        path: &ModelPath,
        options: &ModelLoadOptions,
    ) -> Result<LoadedModel, Report<ModelLoadError>> {
        let db = &self.assets.db();
        let sources = self.read_parts(db, path)?;
        let group = self.part_group(path);

        let mut parts = Vec::with_capacity(sources.len());
        let mut meshes = Vec::new();
        let mut armor = Vec::new();
        for source in &sources {
            let geom = geometry::parse_geometry(&source.geometry_bytes)
                .context_with(|| ModelLoadError::GeometryParse { path: source.geometry_path.clone() })?;
            // A part clamps a deep LOD request to its own deepest, as export does.
            let lod = options.lod.min(source.visual.lods.len().saturating_sub(1));
            let mut part_meshes = gltf_export::collect_hull_meshes_indexed(
                &source.visual,
                &geom,
                db,
                &self.self_id_index,
                lod,
                options.damaged,
                None,
            )
            .context_with(|| ModelLoadError::Meshes { visual_path: source.visual_path.clone() })?;

            let mesh_transform = mesh_to_node_space(&source.visual, &db.strings).map(negate_z_transform);
            for mesh in &mut part_meshes {
                mesh.transform = mesh_transform;
                mesh.group = group;
            }
            meshes.extend(part_meshes);
            armor.extend(geom.armor_models);

            parts.push(ModelPart {
                model_path: source.model_path.clone(),
                visual_path: source.visual_path.clone(),
                geometry_path: source.geometry_path.clone(),
                lod_count: source.visual.lods.len(),
            });
        }

        let hardpoints = model_hardpoints(db, &sources);

        let mut materials = BTreeMap::new();
        for mesh in &meshes {
            let Some(mfm_path) = &mesh.mfm_path else {
                continue;
            };
            if materials.contains_key(mfm_path) {
                continue;
            }
            let material = self.resolve_material(db, mfm_path, mesh.mfm_path_id, options);
            materials.insert(mfm_path.clone(), material);
        }

        Ok(LoadedModel { path: path.clone(), parts, meshes, materials, hardpoints, armor })
    }

    /// Writes `path` with its sections as a GLB: one mesh node per part with
    /// albedo textures, the armor split by zone with thickness from `armor`
    /// when `contents` asks for it, and one empty node per hardpoint, named as
    /// the node, when `hardpoints` asks for them. Reads the VFS and encodes
    /// textures, so run it off the UI thread.
    pub fn export_glb(
        &self,
        path: &ModelPath,
        options: &ModelExportOptions,
        armor: ModelArmorMaps<'_>,
        writer: &mut impl Write,
    ) -> Result<(), Report<ModelExportError>> {
        let db = &self.assets.db();
        let sources = self.read_parts(db, path).context_with(|| ModelExportError::Load { path: path.clone() })?;
        let group = self.part_group(path);

        let mut geometries = Vec::with_capacity(sources.len());
        for source in &sources {
            let geometry = geometry::parse_geometry(&source.geometry_bytes)
                .context_with(|| ModelExportError::GeometryParse { path: source.geometry_path.clone() })?;
            geometries.push(geometry);
        }

        let sub_models: Vec<SubModel<'_>> = sources
            .iter()
            .zip(&geometries)
            .map(|(source, geometry)| SubModel {
                name: source.model_path.stem().to_string(),
                visual: &source.visual,
                geometry,
                transform: mesh_to_node_space(&source.visual, &db.strings),
                group,
                barrel_pitch: None,
            })
            .collect();

        let textures = if options.textures && options.contents.includes_mesh() {
            self.export_textures(db, &sources, &geometries, options)?
        } else {
            TextureSet::empty()
        };

        let mut armor_meshes = Vec::new();
        if options.contents.includes_armor() {
            for armor_model in geometries.iter().flat_map(|geometry| &geometry.armor_models) {
                armor_meshes.extend(gltf_export::armor_sub_models_by_zone(armor_model, armor.hull, armor.mount));
            }
        }

        let hardpoints = if options.hardpoints { model_hardpoints(db, &sources) } else { Vec::new() };
        let hardpoint_sets = [HardpointSet { name: path.stem().to_string(), hardpoints: &hardpoints }];

        gltf_export::export_ship_glb(
            &sub_models,
            &armor_meshes,
            db,
            options.contents,
            options.lod,
            &textures,
            options.damaged,
            &hardpoint_sets,
            writer,
        )
        .context_with(|| ModelExportError::Write { path: path.clone() })
    }

    /// The model itself, then every section the catalogue lists for it.
    fn read_parts(
        &self,
        db: &PrototypeDatabase<'_>,
        path: &ModelPath,
    ) -> Result<Vec<PartSource>, Report<ModelLoadError>> {
        let sections = self.catalog.entry(path).into_iter().flat_map(ModelEntry::sections);
        let part_paths: Vec<&ModelPath> = std::iter::once(path).chain(sections).collect();

        let mut sources = Vec::with_capacity(part_paths.len());
        for part_path in part_paths {
            sources.push(self.read_part(db, part_path)?);
        }

        Ok(sources)
    }

    fn part_group(&self, path: &ModelPath) -> PartGroup {
        self.catalog.owning_entry(path).map_or(PartGroup::Other, ModelEntry::part_group)
    }

    /// Albedo PNGs by material stem, as the GLB writer keys them, for the
    /// materials of the render sets the export writes at `options`' LOD and
    /// damage state. A material without an albedo the VFS holds stays
    /// untextured.
    fn export_textures(
        &self,
        db: &PrototypeDatabase<'_>,
        sources: &[PartSource],
        geometries: &[geometry::MergedGeometry<'_>],
        options: &ModelExportOptions,
    ) -> Result<TextureSet, Report<ModelExportError>> {
        let mut written = HashSet::new();
        for (source, geometry) in sources.iter().zip(geometries) {
            // A part clamps a deep LOD request to its own deepest, as the writer does.
            let Some(lod) = source.visual.lods.get(options.lod).or_else(|| source.visual.lods.last()) else {
                continue;
            };
            let summary = gltf_export::primitive_summary_from_metadata(
                &source.visual,
                geometry,
                db,
                &self.self_id_index,
                lod,
                options.damaged,
            )
            .context_with(|| ModelExportError::RenderSets { visual_path: source.visual_path.clone() })?;
            written.extend(summary.mfm_stems);
        }

        let load_options = ModelLoadOptions {
            texture_lod: options.texture_lod,
            textures: TextureDecode::Albedo,
            ..ModelLoadOptions::default()
        };
        let mut textures = TextureSet::empty();

        let render_sets = sources.iter().flat_map(|source| &source.visual.render_sets);
        for mfm_path_id in render_sets.map(|render_set| render_set.material_mfm_path_id) {
            let Some(&position) = self.self_id_index.get(&mfm_path_id) else {
                continue;
            };
            let leaf = &db.paths_storage[position].name;
            let stem = leaf.strip_suffix(".mfm").unwrap_or(leaf);
            if !written.contains(stem) || textures.base.contains_key(stem) {
                continue;
            }

            let mfm_path = db.reconstruct_path(position, &self.self_id_index);
            let material = self.resolve_material(db, &mfm_path, mfm_path_id, &load_options);
            let Some(image) = material.albedo.and_then(|albedo| albedo.image) else {
                continue;
            };
            let png =
                encode_png(image).context_with(|| ModelExportError::TextureEncode { mfm_path: mfm_path.clone() })?;
            textures.base.insert(stem.to_string(), png);
        }

        Ok(textures)
    }

    fn read_part(&self, db: &PrototypeDatabase<'_>, path: &ModelPath) -> Result<PartSource, Report<ModelLoadError>> {
        let model = self.read_model_prototype(db, path)?;
        let (visual_path, visual) = self.read_visual(db, path, &model)?;

        let geometry_id = visual.merged_geometry_path_id;
        let geometry_path = self
            .self_id_index
            .get(&geometry_id)
            .map(|&position| db.reconstruct_path(position, &self.self_id_index))
            .ok_or_else(|| {
                Report::new(ModelLoadError::UnresolvedGeometry { visual_path: visual_path.clone(), geometry_id })
            })?;
        let read_error = || ModelLoadError::GeometryRead { path: geometry_path.clone() };
        let geometry_file = self.vfs.join(&geometry_path).context_with(read_error)?;
        if !geometry_file.is_file().context_with(read_error)? {
            return Err(Report::new(ModelLoadError::GeometryAbsent { path: geometry_path.clone() }));
        }
        let mut geometry_bytes = Vec::new();
        geometry_file
            .open_file()
            .context_with(read_error)?
            .read_to_end(&mut geometry_bytes)
            .context_with(read_error)?;

        let extenders = model_extenders(db, &model);

        Ok(PartSource { model_path: path.clone(), visual_path, visual, geometry_path, geometry_bytes, extenders })
    }

    /// The visual record `model` names, with its path. Reads no geometry.
    fn read_visual(
        &self,
        db: &PrototypeDatabase<'_>,
        path: &ModelPath,
        model: &ModelPrototype,
    ) -> Result<(String, VisualPrototype), Report<ModelLoadError>> {
        const VISUAL_BLOB_INDEX: usize = 1;

        let visual_id = model.visual_resource_id;
        if visual_id == 0 {
            return Err(Report::new(ModelLoadError::MissingVisual { path: path.clone() }));
        }
        let visual_error = || ModelLoadError::VisualRecord { path: path.clone(), visual_id };
        let visual_location = db.lookup_r2p(visual_id).ok_or_else(|| Report::new(visual_error()))?;
        let visual_location = db.decode_r2p_value(visual_location).context_with(visual_error)?;
        if visual_location.blob_index != VISUAL_BLOB_INDEX {
            return Err(Report::new(visual_error()));
        }
        let visual_record =
            db.get_prototype_data(visual_location, visual::VISUAL_ITEM_SIZE).context_with(visual_error)?;
        let visual = visual::parse_visual(visual_record).context_with(visual_error)?;
        let visual_path = self
            .self_id_index
            .get(&visual_id)
            .map(|&position| db.reconstruct_path(position, &self.self_id_index))
            .ok_or_else(|| Report::new(visual_error()))?;

        Ok((visual_path, visual))
    }

    /// The visual of the model at `path`, `None` when either record does not read.
    pub(crate) fn model_visual(&self, db: &PrototypeDatabase<'_>, path: &ModelPath) -> Option<VisualPrototype> {
        let model = self.read_model_prototype(db, path).ok()?;

        self.read_visual(db, path, &model).ok().map(|(_, visual)| visual)
    }

    fn read_model_prototype(
        &self,
        db: &PrototypeDatabase<'_>,
        path: &ModelPath,
    ) -> Result<ModelPrototype, Report<ModelLoadError>> {
        let position = self
            .model_index
            .position(path)
            .ok_or_else(|| Report::new(ModelLoadError::UnknownModel { path: path.clone() }))?;
        let record_error = || ModelLoadError::ModelRecord { path: path.clone() };
        let self_id = db.paths_storage[position].self_id;
        let location = db.lookup_r2p(self_id).ok_or_else(|| Report::new(record_error()))?;
        let location = db.decode_r2p_value(location).context_with(record_error)?;
        let record = db.get_prototype_data(location, model::MODEL_ITEM_SIZE).context_with(record_error)?;

        model::parse_model(record).context_with(record_error)
    }

    fn resolve_material(
        &self,
        db: &PrototypeDatabase<'_>,
        mfm_path: &str,
        mfm_path_id: u64,
        options: &ModelLoadOptions,
    ) -> ModelMaterial {
        let material = texture::parse_mfm_from_db(db, mfm_path_id);
        let decode_albedo = options.textures != TextureDecode::None;
        let decode_maps = options.textures == TextureDecode::AllMaps;

        let albedo = self.resolve_albedo(db, material.as_ref(), mfm_path, decode_albedo, options.texture_lod);
        let normal = material
            .as_ref()
            .and_then(|m| self.referenced_texture(db, m, "normalMap", decode_maps, options.texture_lod));
        let metal_gloss = material
            .as_ref()
            .and_then(|m| self.referenced_texture(db, m, "metallicGlossMap", decode_maps, options.texture_lod));

        ModelMaterial { mfm_path: mfm_path.to_string(), albedo, normal, metal_gloss }
    }

    /// The albedo a material shows: a baked TILEDLAND composite, else its
    /// `diffuseMap`, else the texture named after the material.
    fn resolve_albedo(
        &self,
        db: &PrototypeDatabase<'_>,
        material: Option<&MaterialPrototype>,
        mfm_path: &str,
        decode: bool,
        lod: TextureLod,
    ) -> Option<MaterialTexture> {
        if let Some(material) = material
            && texture::is_tiledland_material(material)
        {
            if !decode {
                return Some(MaterialTexture { source: TextureSource::TiledLandBake, image: None });
            }
            if let Some(baked) = texture::bake_tiledland_image(material, &self.vfs, db, &self.self_id_index, lod) {
                let image = RgbaImageData { width: baked.width(), height: baked.height(), pixels: baked.into_raw() };
                return Some(MaterialTexture { source: TextureSource::TiledLandBake, image: Some(image) });
            }
        }

        let mut albedo =
            material.and_then(|m| self.referenced_texture(db, m, "diffuseMap", decode, lod)).or_else(|| {
                let path = texture::base_albedo_path(&self.vfs, mfm_path)?;
                load_texture(&self.vfs, TextureSource::NameMatch { path }, decode, lod)
            })?;

        // Albedo alpha carries other data (height, roughness), not opacity.
        if let Some(image) = &mut albedo.image {
            for pixel in image.pixels.chunks_exact_mut(4) {
                pixel[3] = u8::MAX;
            }
        }

        Some(albedo)
    }

    /// The texture `material` names under `property`, when the VFS holds it.
    fn referenced_texture(
        &self,
        db: &PrototypeDatabase<'_>,
        material: &MaterialPrototype,
        property: &str,
        decode: bool,
        lod: TextureLod,
    ) -> Option<MaterialTexture> {
        let hash = material.get_texture_hash(property)?;
        let &position = self.self_id_index.get(&hash)?;
        let path = db.reconstruct_path(position, &self.self_id_index);

        load_texture(&self.vfs, TextureSource::MaterialProperty { path }, decode, lod)
    }
}

/// A model loaded with its sections, ready to display.
pub struct LoadedModel {
    path: ModelPath,
    parts: Vec<ModelPart>,
    meshes: Vec<InteractiveHullMesh>,
    materials: BTreeMap<String, ModelMaterial>,
    hardpoints: Vec<Hardpoint>,
    armor: Vec<ArmorModel>,
}

impl LoadedModel {
    pub fn path(&self) -> &ModelPath {
        &self.path
    }

    /// The model itself, then its sections.
    pub fn parts(&self) -> &[ModelPart] {
        &self.parts
    }

    /// One mesh per render set of the requested LOD, across all parts.
    /// `transform` is set when a part's meshes need moving into node space.
    pub fn meshes(&self) -> &[InteractiveHullMesh] {
        &self.meshes
    }

    /// Materials by the `.mfm` path [`InteractiveHullMesh::mfm_path`] names.
    pub fn materials(&self) -> &BTreeMap<String, ModelMaterial> {
        &self.materials
    }

    pub fn material(&self, mfm_path: &str) -> Option<&ModelMaterial> {
        self.materials.get(mfm_path)
    }

    pub fn hardpoints(&self) -> &[Hardpoint] {
        &self.hardpoints
    }

    /// The armor models the geometry carries, unplaced and left-handed as
    /// parsed. Use [`Self::armor_meshes`] for display.
    pub fn armor_models(&self) -> &[ArmorModel] {
        &self.armor
    }

    pub fn has_armor(&self) -> bool {
        !self.armor.is_empty()
    }

    /// The most LODs any part authors.
    pub fn lod_count(&self) -> usize {
        self.parts.iter().map(|p| p.lod_count).max().unwrap_or(0)
    }

    /// Armor meshes with thickness from `mount_armor`, falling back to
    /// `hull_armor` per material, as a ship shows them. Without either map
    /// every plate reads 0 mm.
    pub fn armor_meshes(
        &self,
        hull_armor: Option<&ArmorMap>,
        mount_armor: Option<&ArmorMap>,
    ) -> Vec<InteractiveArmorMesh> {
        self.armor.iter().map(|armor| InteractiveArmorMesh::from_armor_model(armor, hull_armor, mount_armor)).collect()
    }

    /// Takes the loaded data apart without copying meshes or textures.
    pub fn into_parts(self) -> LoadedModelParts {
        let Self { path, parts, meshes, materials, hardpoints, armor } = self;

        LoadedModelParts { path, parts, meshes, materials, hardpoints, armor }
    }
}

/// The contents of a [`LoadedModel`], as [`LoadedModel::into_parts`] returns them.
pub struct LoadedModelParts {
    pub path: ModelPath,
    pub parts: Vec<ModelPart>,
    pub meshes: Vec<InteractiveHullMesh>,
    pub materials: BTreeMap<String, ModelMaterial>,
    pub hardpoints: Vec<Hardpoint>,
    pub armor: Vec<ArmorModel>,
}

/// What [`ModelLibrary::model_summary`] reads of a model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelSummary {
    /// The most LODs any part authors.
    pub lod_count: usize,
    pub has_armor: bool,
}

/// One `.model` that contributed meshes to a [`LoadedModel`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelPart {
    pub model_path: ModelPath,
    pub visual_path: String,
    pub geometry_path: String,
    pub lod_count: usize,
}

/// The textures one material references.
#[derive(Clone)]
pub struct ModelMaterial {
    pub mfm_path: String,
    pub albedo: Option<MaterialTexture>,
    pub normal: Option<MaterialTexture>,
    pub metal_gloss: Option<MaterialTexture>,
}

#[derive(Clone)]
pub struct MaterialTexture {
    pub source: TextureSource,
    /// RGBA8 at the requested texture LOD, when [`TextureDecode`] asked for it.
    pub image: Option<RgbaImageData>,
}

/// Where a texture came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextureSource {
    /// The `.dds` the material references by property.
    MaterialProperty { path: String },
    /// The `.dds` found by the material's file name, for a material whose
    /// albedo property is absent or names a file the VFS lacks.
    NameMatch { path: String },
    /// Composited from a TILEDLAND material's tile atlas and blend map.
    TiledLandBake,
}

impl TextureSource {
    /// The VFS path of the texture file, absent for a baked texture.
    pub fn path(&self) -> Option<&str> {
        match self {
            Self::MaterialProperty { path } | Self::NameMatch { path } => Some(path),
            Self::TiledLandBake => None,
        }
    }
}

/// What one part's load reads before any decoding.
struct PartSource {
    model_path: ModelPath,
    visual_path: String,
    visual: VisualPrototype,
    geometry_path: String,
    geometry_bytes: Vec<u8>,
    extenders: Vec<SkeletonExtender>,
}

/// The `HP_` nodes of every part's visual and the `MP_`/`EP_` nodes of their
/// skeleton extenders, in the model's right-handed node space. Sections reuse
/// node names (each end of a hull has its own bollards), so each part keeps
/// its own; a node the ship model and a section both carry at the same place
/// is kept once.
fn model_hardpoints(db: &PrototypeDatabase<'_>, sources: &[PartSource]) -> Vec<Hardpoint> {
    let visuals: Vec<&VisualPrototype> = sources.iter().map(|s| &s.visual).collect();
    let mut hardpoints: Vec<Hardpoint> = Vec::new();
    for source in sources {
        let mut part = ModelSpaceHardpoints::new();
        part.add_visual(&source.visual, &db.strings);
        part.add_extenders(&source.extenders, &source.visual, &visuals, &db.strings);
        for hardpoint in part.placed(None) {
            if !hardpoints.contains(&hardpoint) {
                hardpoints.push(hardpoint);
            }
        }
    }

    hardpoints
}

#[derive(Debug, Error)]
enum PngEncodeError {
    #[error("{pixels} bytes do not fill a {width}x{height} RGBA image")]
    Dimensions { width: u32, height: u32, pixels: usize },
    #[error("the PNG encoder failed")]
    Encode,
}

fn encode_png(image: RgbaImageData) -> Result<Vec<u8>, Report<PngEncodeError>> {
    let RgbaImageData { width, height, pixels } = image;
    let pixel_bytes = pixels.len();
    let buffer = image::RgbaImage::from_raw(width, height, pixels)
        .ok_or_else(|| Report::new(PngEncodeError::Dimensions { width, height, pixels: pixel_bytes }))?;

    let mut png = std::io::Cursor::new(Vec::new());
    buffer.write_to(&mut png, image::ImageFormat::Png).context(PngEncodeError::Encode)?;

    Ok(png.into_inner())
}

/// The texture at `source`, decoded when asked, or `None` when the VFS lacks
/// the file or it does not decode.
fn load_texture(vfs: &VfsPath, source: TextureSource, decode: bool, lod: TextureLod) -> Option<MaterialTexture> {
    let path = source.path()?;
    if !decode {
        return texture::vfs_file_exists(vfs, path).then_some(MaterialTexture { source, image: None });
    }

    let bytes = texture::load_dds_from_vfs(vfs, path, lod)?;
    let decoded = texture::decode_dds(&bytes, lod).ok()?;
    let image = RgbaImageData { width: decoded.width(), height: decoded.height(), pixels: decoded.into_raw() };

    Some(MaterialTexture { source, image: Some(image) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_library_can_be_shared_with_background_loads() {
        fn shareable<T: Send + Sync>() {}

        shareable::<ModelLibrary>();
        shareable::<LoadedModel>();
    }
}
