//! High-level ship model export API.
//!
//! Provides [`ShipAssets`] (shared expensive resources, created once) and
//! [`ShipModelContext`] (a fully-loaded ship, ready for GLB export).
//!
//! # Quick start
//! ```no_run
//! use wowsunpack::export::ship::{ShipAssets, ShipExportOptions};
//! # fn main() -> rootcause::Result<()> {
//! # let vfs: vfs::VfsPath = todo!();
//! let assets = ShipAssets::load(&vfs)?;
//! let ctx = assets.load_ship("Yamato", &ShipExportOptions::default())?;
//! let mut file = std::fs::File::create("yamato.glb")?;
//! ctx.export_glb(&mut file)?;
//! # Ok(())
//! # }
//! ```

use std::collections::HashMap;
use std::collections::HashSet;
use std::io::Read;
use std::io::Write;
use std::path::Path;
use std::sync::Arc;

use rootcause::prelude::*;
use vfs::VfsPath;

use crate::data::ResourceLoader;
use crate::data::TranslationKey;
use crate::game_params::keys;
use crate::game_params::provider::GameMetadataProvider;
use crate::game_params::types::ArmorMap;
use crate::game_params::types::GameParamProvider;
use crate::game_params::types::HullUpgradeConfig;
use crate::game_params::types::MountPoint;
use crate::game_params::types::Species;
use crate::game_params::types::Vehicle;
use crate::models::assets_bin;
use crate::models::assets_bin::PrototypeDatabase;
use crate::models::geometry;
use crate::models::model;
use crate::models::skeleton_extender;
use crate::models::visual;
use crate::models::visual::VisualPrototype;
use crate::recognized::Recognized;

use super::camo_composite;
use super::camo_textures;
use super::camo_textures::CamoSchemeId;
use super::camouflage;
use super::camouflage::CamouflageDb;
use super::camouflage::UvTransform;
use super::gltf_export;
use super::gltf_export::HardpointSet;
use super::gltf_export::InteractiveArmorMesh;
use super::gltf_export::SubModel;
use super::gltf_export::TextureSet;
use super::gltf_export::negate_z_transform;
use super::hardpoint::HardpointName;
use super::hardpoint::ModelSpaceHardpoints;
use super::hardpoint::mat4_mul;
use super::hardpoint::mesh_to_node_space;
use super::hardpoint::model_extenders;
use super::material_alpha::AlphaMode;
use super::model_catalog::ModelPath;
use super::model_library::ModelLibrary;
use super::model_library::ModelLibraryError;
use super::part_group::PartGroup;
use super::ship_parts::MISC_NODE_PREFIX;
use super::ship_parts::MiscParent;
use super::ship_parts::ShipPart;
use super::ship_parts::ShipPartKey;
use super::ship_parts::battle_section_extenders;
use super::ship_parts::misc_name_from_node;
use super::ship_parts::mount_has_misc_selection;
use super::ship_parts::mount_shows_misc;
use super::ship_parts::select_hull_mount_points;
use super::ship_parts::select_hull_upgrade;
use super::ship_parts::split_compound_hp;
use super::size_estimate;
use super::texture;
use super::texture::MaterialAlbedo;

/// What geometry the exported GLB carries. Armor is a per-zone triangle soup for
/// the hull and every mount, so a caller who wants the visual model should not
/// pay to tessellate and embed it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ExportContents {
    #[default]
    Mesh,
    Armor,
    MeshAndArmor,
}

impl ExportContents {
    pub fn includes_mesh(self) -> bool {
        matches!(self, Self::Mesh | Self::MeshAndArmor)
    }

    pub fn includes_armor(self) -> bool {
        matches!(self, Self::Armor | Self::MeshAndArmor)
    }
}

/// Which camouflage schemes reach the GLB. Ids index the ordered list
/// [`ShipModelContext::camo_texture_source`] enumerates, which is the same list
/// the viewer's picker reads.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum CamoSelection {
    /// Base albedo only. No `KHR_materials_variants` extension is emitted.
    #[default]
    BaseOnly,
    /// Base material plus one variant material set per scheme.
    Variants(Vec<CamoSchemeId>),
    /// One scheme composited into the base material. No variants extension.
    Baked(CamoSchemeId),
}

impl CamoSelection {
    /// Scheme ids [`size_estimate::ExportSizeModel`] must have ladders for to
    /// price this selection. `Baked` prices from the model's `base_ladders`
    /// alone (a baked camo replaces each stem's base albedo at the base's own
    /// resolution, so it costs what stock costs -- see
    /// `ExportSizeModel::texture_bytes`), so despite carrying an id it needs
    /// nothing here.
    pub(crate) fn priced_scheme_ids(&self) -> Vec<CamoSchemeId> {
        match self {
            CamoSelection::BaseOnly | CamoSelection::Baked(_) => Vec::new(),
            CamoSelection::Variants(ids) => ids.clone(),
        }
    }
}

/// Options controlling ship model export.
#[derive(Debug, Clone)]
pub struct ShipExportOptions {
    /// LOD level (0 = highest detail). Default: 0.
    pub lod: usize,
    /// Hull upgrade selection. `None` = first/stock hull.
    /// Accepts full upgrade name (e.g. "PJUH911_Yamato_1944") or a prefix
    /// match against the hull component name (e.g. "B").
    pub hull: Option<String>,
    /// Whether to embed textures in the GLB. Default: true.
    pub textures: bool,
    /// Export the damaged/destroyed hull state instead of intact.
    /// When true, crack geometry is included and patch geometry is excluded.
    /// Default: false (intact hull).
    pub damaged: bool,
    /// What geometry reaches the GLB. Default: [`ExportContents::Mesh`].
    /// Does not affect [`ShipModelContext::interactive_armor_meshes`], which
    /// serves armor to viewers independently.
    pub contents: ExportContents,
    /// Detail level for textures embedded in the GLB. Default: [`TextureLod::Full`].
    pub texture_lod: texture::TextureLod,
    /// Module overrides: component type key (e.g. "artillery") to component name.
    /// Overrides the default component for specific types.
    pub module_overrides: std::collections::HashMap<crate::game_params::keys::ComponentType, String>,
    /// Which camouflage schemes to embed. Default: [`CamoSelection::BaseOnly`].
    pub camos: CamoSelection,
    /// Write one empty node per part hardpoint. Default: true.
    pub hardpoints: bool,
}

impl Default for ShipExportOptions {
    fn default() -> Self {
        Self {
            lod: 0,
            hull: None,
            textures: true,
            damaged: false,
            contents: ExportContents::Mesh,
            texture_lod: texture::TextureLod::Full,
            module_overrides: std::collections::HashMap::new(),
            camos: CamoSelection::BaseOnly,
            hardpoints: true,
        }
    }
}

/// Resolved ship identity information.
#[derive(Debug, Clone)]
pub struct ShipInfo {
    /// Model directory name, e.g. "JSB039_Yamato_1945".
    pub model_dir: String,
    /// Translated display name if translations are loaded, e.g. "Yamato".
    pub display_name: Option<String>,
    /// GameParam index key, e.g. "PJSB018".
    pub param_index: String,
}

/// Summary of a hull upgrade for listing purposes.
#[derive(Debug, Clone)]
pub struct HullUpgradeInfo {
    /// Upgrade name (GameParam key), e.g. "PJUH911_Yamato_1944".
    pub name: String,
    /// Components in this upgrade: (type_key, component_name, mount_count).
    pub components: Vec<(String, String, usize)>,
}

// ---------------------------------------------------------------------------
// ShipAssets — shared expensive resources (created once)
// ---------------------------------------------------------------------------

/// Shared game assets for ship export operations.
///
/// Creating this is the expensive step (~18 seconds for GameParams parsing).
/// Reuse a single instance across multiple ship exports.
pub struct ShipAssets {
    /// Shared via `Arc` so `ShipModelContext::assets_bin_bytes` (cloned once per
    /// ship load) is a refcount bump, not a ~170 MB memcpy.
    assets_bin_bytes: Arc<Vec<u8>>,
    vfs: VfsPath,
    metadata: Arc<GameMetadataProvider>,
    camo_db: Option<CamouflageDb>,
}

impl ShipAssets {
    /// Load shared assets from the VFS.
    ///
    /// This is expensive (~18 seconds) because it parses GameParams.
    /// Create once and reuse for multiple ships.
    pub fn load(vfs: &VfsPath) -> Result<Self, Report> {
        let mut assets_bin_bytes = Vec::new();
        vfs.join("content/assets.bin")
            .context("VFS path error")?
            .open_file()
            .context("Could not find content/assets.bin in VFS")?
            .read_to_end(&mut assets_bin_bytes)?;

        let metadata = Arc::new(GameMetadataProvider::from_vfs(vfs).context("Failed to load GameParams")?);

        let camo_db = CamouflageDb::load(vfs);

        Ok(Self { assets_bin_bytes: Arc::new(assets_bin_bytes), vfs: vfs.clone(), metadata, camo_db })
    }

    /// Load shared assets from the VFS, reusing an already-loaded [`GameMetadataProvider`].
    ///
    /// This skips the expensive GameParams parse that [`Self::load`] performs,
    /// making it suitable when the caller already has metadata available.
    pub fn from_vfs_with_metadata(vfs: &VfsPath, metadata: Arc<GameMetadataProvider>) -> Result<Self, Report> {
        let mut assets_bin_bytes = Vec::new();
        vfs.join("content/assets.bin")
            .context("VFS path error")?
            .open_file()
            .context("Could not find content/assets.bin in VFS")?
            .read_to_end(&mut assets_bin_bytes)?;

        Ok(Self::from_parts(vfs, metadata, Arc::new(assets_bin_bytes)))
    }

    /// The same, over an `assets.bin` blob the caller already holds.
    ///
    /// A caller whose VFS carries the `assets.bin` overlay already has the blob
    /// (`game_data::AssetsOverlay::bytes`); reading it again would hold 174 MiB
    /// of it twice.
    pub fn from_parts(vfs: &VfsPath, metadata: Arc<GameMetadataProvider>, assets_bin_bytes: Arc<Vec<u8>>) -> Self {
        let camo_db = CamouflageDb::load(vfs);
        Self { assets_bin_bytes, vfs: vfs.clone(), metadata, camo_db }
    }

    /// Load shared assets directly from a World of Warships installation directory.
    ///
    /// This is a convenience wrapper that builds the VFS (idx files + assets.bin overlay)
    /// from the game directory, then calls [`Self::load`]. It uses the latest build
    /// found in the `bin/` directory.
    ///
    /// For callers who already have a VFS, use [`Self::load`] instead.
    pub fn from_game_dir(game_dir: &Path) -> Result<Self, Report> {
        let vfs = crate::game_data::build_game_vfs(game_dir, crate::game_data::AssetsBin::Overlay)?;
        Self::load(&vfs)
    }

    /// Set translations for display name resolution.
    pub fn set_translations(&self, catalog: gettext::Catalog) {
        self.metadata.set_translations(catalog);
    }

    /// Access the underlying `GameMetadataProvider`.
    pub fn metadata(&self) -> &GameMetadataProvider {
        &self.metadata
    }

    /// Access the underlying VFS root.
    pub fn vfs(&self) -> &VfsPath {
        &self.vfs
    }

    /// Build the model catalogue and loader over these assets, sharing the
    /// `assets.bin` blob. Parses `assets.bin` once; build it once and keep it.
    pub fn model_library(&self) -> Result<ModelLibrary, Report<ModelLibraryError>> {
        ModelLibrary::new(self.vfs.clone(), Arc::clone(&self.assets_bin_bytes), &self.metadata)
    }

    /// Find a ship by name (fuzzy display-name match or exact model dir).
    pub fn find_ship(&self, name: &str) -> Result<ShipInfo, Report> {
        let db = self.db()?;
        let self_id_index = db.build_self_id_index();

        // Strategy 1: try direct match against assets.bin paths.
        let needle = format!("/{name}/");
        let has_direct = db.paths_storage.iter().any(|e| {
            e.name.ends_with(".visual") && {
                // Reconstruct is expensive; just check if any visual file's
                // full path contains the needle. We only need one hit.
                let idx = db.paths_storage.iter().position(|x| std::ptr::eq(x, e)).unwrap();
                db.reconstruct_path(idx, &self_id_index).contains(&needle)
            }
        });

        if has_direct {
            // Direct model dir match — no GameParams needed for identity.
            // Try to find the GameParam for richer info.
            let param = self
                .metadata
                .params()
                .iter()
                .find(|p| p.vehicle().and_then(|v| v.model_path()).map(|mp| mp.contains(name)).unwrap_or(false));

            return Ok(ShipInfo {
                model_dir: name.to_string(),
                display_name: param.and_then(|p| self.metadata.localized_name_from_param(p)),
                param_index: param.map(|p| p.index().to_string()).unwrap_or_default(),
            });
        }

        // Strategy 2: exact param index match via GameParams.
        if let Some(param) = self.metadata.game_param_by_index(name)
            && let Some(vehicle) = param.vehicle()
            && let Some(model_path) = vehicle.model_path()
        {
            let dir = model_path.rsplit_once('/').map(|(d, _)| d).unwrap_or(model_path);
            let model_dir = dir.rsplit('/').next().unwrap_or(dir);
            return Ok(ShipInfo {
                model_dir: model_dir.to_string(),
                display_name: self.metadata.localized_name_from_param(&param),
                param_index: param.index().to_string(),
            });
        }

        // Strategy 3: fuzzy display name match via GameParams.
        let normalized_input = unidecode::unidecode(name).to_lowercase();
        let mut matches: Vec<(String, String, String)> = Vec::new();

        for param in self.metadata.params() {
            let vehicle = match param.vehicle() {
                Some(v) => v,
                None => continue,
            };
            let model_path = match vehicle.model_path() {
                Some(p) => p,
                None => continue,
            };

            let display_name =
                self.metadata.localized_name_from_param(param).unwrap_or_else(|| param.index().to_string());

            let normalized_display = unidecode::unidecode(&display_name).to_lowercase();
            if normalized_display.contains(&normalized_input) {
                let dir = model_path.rsplit_once('/').map(|(d, _)| d).unwrap_or(model_path);
                let dir_name = dir.rsplit('/').next().unwrap_or(dir);
                matches.push((display_name, param.index().to_string(), dir_name.to_string()));
            }
        }

        match matches.len() {
            0 => bail!(
                "No ship found matching '{name}'. Try using the model directory name \
                 (e.g. 'JSB039_Yamato_1945')."
            ),
            1 => Ok(ShipInfo {
                model_dir: matches[0].2.clone(),
                display_name: Some(matches[0].0.clone()),
                param_index: matches[0].1.clone(),
            }),
            _ => {
                // If all matches share the same model dir, use it.
                let unique_dirs: HashSet<&str> = matches.iter().map(|(_, _, d)| d.as_str()).collect();
                if unique_dirs.len() == 1 {
                    return Ok(ShipInfo {
                        model_dir: matches[0].2.clone(),
                        display_name: Some(matches[0].0.clone()),
                        param_index: matches[0].1.clone(),
                    });
                }

                let listing: Vec<String> =
                    matches.iter().map(|(display, idx, dir)| format!("  {display} ({idx}) -> {dir}")).collect();
                bail!(
                    "Multiple ships match '{name}':\n{}\nPlease refine your search \
                     or use the model directory name directly.",
                    listing.join("\n")
                );
            }
        }
    }

    /// List hull upgrades for a ship.
    pub fn list_hull_upgrades(&self, name: &str) -> Result<Vec<HullUpgradeInfo>, Report> {
        let info = self.find_ship(name)?;
        let vehicle = self.find_vehicle(&info.model_dir)?;

        let Some(upgrades) = vehicle.hull_upgrades() else {
            return Ok(Vec::new());
        };

        let mut result = Vec::new();
        let mut sorted: Vec<_> = upgrades.iter().collect();
        sorted.sort_by_key(|(k, _)| (*k).clone());

        for (upgrade_name, config) in sorted {
            let mut components = Vec::new();
            for ct in keys::ComponentType::ALL {
                let comp = config.component_name(*ct).unwrap_or("(none)").to_string();
                let mount_count = config.mounts(*ct).map(|m| m.len()).unwrap_or(0);
                components.push((ct.to_string(), comp, mount_count));
            }
            result.push(HullUpgradeInfo { name: upgrade_name.clone(), components });
        }

        Ok(result)
    }

    /// Camouflage scheme names for a ship, as `--camo` accepts them.
    pub fn list_texture_schemes(&self, name: &str) -> Result<Vec<String>, Report> {
        let options = ShipExportOptions { textures: false, ..Default::default() };
        let ctx = self.load_ship(name, &options)?;
        Ok(ctx.camo_texture_source()?.scheme_infos().into_iter().map(|i| i.display_name).collect())
    }

    /// Load a complete ship model, ready for export.
    pub fn load_ship(&self, name: &str, options: &ShipExportOptions) -> Result<ShipModelContext, Report> {
        let info = self.find_ship(name)?;
        let vehicle = self.find_vehicle(&info.model_dir).ok();
        self.load_ship_inner(info, vehicle, options)
    }

    /// Load a ship using a [`Vehicle`] reference instead of a name lookup.
    ///
    /// This is useful when the caller already has a `Vehicle` from their own
    /// GameParams processing and wants to skip the name-based search.
    pub fn load_ship_from_vehicle(
        &self,
        vehicle: &Vehicle,
        options: &ShipExportOptions,
    ) -> Result<ShipModelContext, Report> {
        let model_path = vehicle.model_path().ok_or_else(|| rootcause::report!("Vehicle has no model_path"))?;
        // model_path is like "content/gameplay/nation/ship/DIR_NAME/file.model"
        let dir = model_path.rsplit_once('/').map(|(d, _)| d).unwrap_or(model_path);
        let model_dir = dir.rsplit('/').next().unwrap_or(dir);

        let param = self
            .metadata
            .params()
            .iter()
            .find(|p| p.vehicle().and_then(|v| v.model_path()).map(|mp| mp.contains(model_dir)).unwrap_or(false));

        let info = ShipInfo {
            model_dir: model_dir.to_string(),
            display_name: param.and_then(|p| self.metadata.localized_name_from_param(p)),
            param_index: param.map(|p| p.index().to_string()).unwrap_or_default(),
        };

        self.load_ship_inner(info, Some(vehicle), options)
    }

    fn load_ship_inner(
        &self,
        info: ShipInfo,
        vehicle: Option<&Vehicle>,
        options: &ShipExportOptions,
    ) -> Result<ShipModelContext, Report> {
        let db = self.db()?;
        let self_id_index = db.build_self_id_index();

        // Find all .visual files in the model directory.
        let visual_paths = self.find_visual_paths(&db, &self_id_index, &info.model_dir);
        if visual_paths.is_empty() {
            bail!("No .visual files found for '{}'.", info.model_dir);
        }

        // Load hull sub-models.
        let hull_parts = self.load_sub_models(&db, &self_id_index, &visual_paths)?;

        // Load turret/mount models from GameParams.
        let mount_points: Vec<MountPoint> = vehicle
            .and_then(|v| select_hull_mount_points(v, options.hull.as_deref(), &options.module_overrides))
            .unwrap_or_default();

        let loaded = self.load_mounts(&db, &self_id_index, &mount_points, &hull_parts)?;
        let turret_models = loaded.turret_models;
        let mounts = loaded.mounts;

        // Load misc parts (propellers, boats, deck fittings) from the skeleton extenders
        // the hull section and mount models name and the hull component adds, and place
        // them at their `MP_` nodes.
        let model_leaves = ModelLeaves::new(&db);
        let upgrade = vehicle.and_then(|v| select_hull_upgrade(v, options.hull.as_deref()));
        let hull =
            HullSections::new(&hull_parts, &visual_paths, &db, &self_id_index, &model_leaves, &info.model_dir, upgrade);
        let (misc_models, miscs) =
            self.collect_miscs(&db, &self_id_index, &model_leaves, &hull, &mounts, &turret_models);

        let parts = ShipPartSources {
            hull: &hull,
            turret_models: &turret_models,
            turret_model_index: &loaded.turret_model_index,
            mounts: &mounts,
            misc_models: &misc_models,
            miscs: &miscs,
        }
        .parts(&db, &self_id_index, &model_leaves);

        // Resolve material-based camouflage schemes (ship-specific + universal + expendable).
        let ship_index = self.find_ship_index(&info.model_dir);
        let ship_idx = ship_index.as_deref();
        let mut mat_camo_schemes = self.discover_mat_camo_schemes(&info.model_dir, ship_idx);
        mat_camo_schemes.extend(self.discover_universal_camo_schemes(ship_idx));
        mat_camo_schemes.extend(self.discover_expendable_camo_schemes(ship_idx));
        {
            let mut seen = HashSet::new();
            mat_camo_schemes.retain(|s| seen.insert(s.camo_name.clone()));
        }

        // Extract armor thickness map and hit locations from GameParams.
        let armor_map = vehicle.and_then(|v| v.armor().cloned());
        let hit_locations = vehicle.and_then(|v| v.hit_locations().cloned());

        Ok(ShipModelContext {
            vfs: self.vfs.clone(),
            assets_bin_bytes: self.assets_bin_bytes.clone(),
            hull_parts,
            turret_models,
            mounts,
            misc_models,
            miscs,
            parts,
            info,
            options: options.clone(),
            mat_camo_schemes,
            armor_map,
            hit_locations,
        })
    }

    // --- Internal helpers ---

    /// Discover material-based camo schemes available for a ship via GameParams.
    ///
    /// Follows: Vehicle.permoflages → Exterior.camouflage → camouflages.xml entry.
    /// Returns owned `MatCamoScheme` data (no lifetimes).
    fn discover_mat_camo_schemes(&self, model_dir: &str, ship_index: Option<&str>) -> Vec<MatCamoScheme> {
        let camo_db = match &self.camo_db {
            Some(db) => db,
            None => return Vec::new(),
        };
        let vehicle = match self.find_vehicle(model_dir) {
            Ok(v) => v,
            Err(_) => return Vec::new(),
        };

        let mut result = Vec::new();
        let mut seen_camo_names = HashSet::new();

        for permo_name in vehicle.permoflages() {
            // permoflages entries are param names (e.g. "PCEM017_Steel_10lvl"), not indices.
            let param =
                self.metadata.game_param_by_name(permo_name).or_else(|| self.metadata.game_param_by_index(permo_name));
            let Some(param) = param else {
                continue;
            };
            // ShipDestruction-species exteriors are death skins, not selectable camos.
            if matches!(param.species(), Some(Recognized::Unknown(s)) if s == "ShipDestruction") {
                continue;
            }
            let Some(exterior) = param.exterior() else {
                continue;
            };
            // Decorative Skin exteriors leave `camouflage` empty and name the texture in
            // `unpeculiarCamouflage` (the unowned-peculiarity appearance); use it so the camo
            // still enumerates instead of being dropped.
            let Some(camo_name) = exterior.camouflage().or_else(|| exterior.unpeculiar_camouflage()) else {
                continue;
            };

            // Deduplicate by camo name (multiple exteriors can share the same camo).
            if !seen_camo_names.insert(camo_name.to_string()) {
                continue;
            }

            let Some(entry) = camo_db.get(camo_name, ship_index) else {
                continue;
            };
            if entry.textures.is_empty() {
                continue;
            }

            // Build display name from translation.
            // Exterior entries use IDS_{NAME_UPPER} as the translation key
            // (e.g. "PCEM017_Steel_10lvl" → "IDS_PCEM017_STEEL_10LVL").
            let ids_key = format!("IDS_{}", permo_name.to_uppercase());
            let display_name = self
                .metadata
                .localized_name_from_id(&TranslationKey::new(ids_key))
                .or_else(|| {
                    // Fallback: try IDS_{index}
                    self.metadata.localized_name_from_param(&param)
                })
                .unwrap_or_else(|| camo_name.to_string());

            // A color scheme means the texture is a zone mask that must be colorized,
            // regardless of `tiled` (which only controls UV repetition).
            let color_scheme_colors =
                entry.color_scheme.as_ref().and_then(|cs_name| camo_db.color_scheme(cs_name)).map(|cs| cs.colors);

            result.push(MatCamoScheme {
                display_name,
                textures: entry.textures.clone(),
                tiled: entry.tiled,
                use_color_scheme: entry.use_color_scheme,
                color_scheme_colors,
                uv_transforms: entry.uv_transforms.clone(),
                camo_name: camo_name.to_string(),
                origin: gltf_export::CamoOrigin::ShipSpecific,
            });
        }

        result
    }

    /// Discover universal camouflage schemes: MSkin-species exteriors flagged
    /// isTileflage (the game's global tile-camo set, available to all ships).
    /// Deduplicated by camouflage name.
    fn discover_universal_camo_schemes(&self, ship_index: Option<&str>) -> Vec<MatCamoScheme> {
        let camo_db = match &self.camo_db {
            Some(db) => db,
            None => return Vec::new(),
        };

        let mut result = Vec::new();
        let mut seen_camo_names = HashSet::new();

        for param in self.metadata.params() {
            let name = param.name();
            // Universal camos are MSkin-species exteriors flagged isTileflage (the game's
            // global tileflage set); the legacy PCEC prefix is the wrong criterion and misses
            // tile camos that lack a PCEC counterpart.
            if !matches!(param.species(), Some(Recognized::Known(Species::MSkin))) {
                continue;
            }
            let Some(exterior) = param.exterior() else {
                continue;
            };
            if !exterior.is_tileflage() {
                continue;
            }
            let Some(camo_name) = exterior.camouflage() else {
                continue;
            };
            if !seen_camo_names.insert(camo_name.to_string()) {
                continue;
            }

            let Some(entry) = camo_db.get(camo_name, ship_index) else {
                continue;
            };
            if entry.textures.is_empty() {
                continue;
            }

            let display_name = self
                .metadata
                .localized_name_from_id(&TranslationKey::new(format!("IDS_{}", name.to_uppercase())))
                .unwrap_or_else(|| camo_name.to_string());

            // A color scheme means the texture is a zone mask that must be colorized,
            // regardless of `tiled` (which only controls UV repetition).
            let color_scheme_colors =
                entry.color_scheme.as_ref().and_then(|cs_name| camo_db.color_scheme(cs_name)).map(|cs| cs.colors);

            result.push(MatCamoScheme {
                display_name,
                textures: entry.textures.clone(),
                tiled: entry.tiled,
                use_color_scheme: entry.use_color_scheme,
                color_scheme_colors,
                uv_transforms: entry.uv_transforms.clone(),
                camo_name: camo_name.to_string(),
                origin: gltf_export::CamoOrigin::Universal,
            });
        }

        result
    }

    /// Discover expendable (consumable) camouflages: species `Camouflage` (the PCEC* paint camos).
    /// These are available to all ships via the exterior slot system, not `permoflages`.
    fn discover_expendable_camo_schemes(&self, ship_index: Option<&str>) -> Vec<MatCamoScheme> {
        let camo_db = match &self.camo_db {
            Some(db) => db,
            None => return Vec::new(),
        };
        let mut result = Vec::new();
        let mut seen_camo_names = HashSet::new();
        for param in self.metadata.params() {
            if !matches!(param.species(), Some(Recognized::Known(Species::Camouflage))) {
                continue;
            }
            let Some(exterior) = param.exterior() else {
                continue;
            };
            // Skip hidden/legacy expendables (removed from the shop) to avoid a 100+ entry dump.
            if exterior.hidden() {
                continue;
            }
            let Some(camo_name) = exterior.camouflage() else {
                continue;
            };
            if !seen_camo_names.insert(camo_name.to_string()) {
                continue;
            }
            let Some(entry) = camo_db.get(camo_name, ship_index) else {
                continue;
            };
            if entry.textures.is_empty() {
                continue;
            }
            let display_name = self
                .metadata
                .localized_name_from_id(&TranslationKey::new(format!("IDS_{}", param.name().to_uppercase())))
                .unwrap_or_else(|| camo_name.to_string());
            let color_scheme_colors =
                entry.color_scheme.as_ref().and_then(|cs| camo_db.color_scheme(cs)).map(|cs| cs.colors);
            result.push(MatCamoScheme {
                display_name,
                textures: entry.textures.clone(),
                tiled: entry.tiled,
                use_color_scheme: entry.use_color_scheme,
                color_scheme_colors,
                uv_transforms: entry.uv_transforms.clone(),
                camo_name: camo_name.to_string(),
                origin: gltf_export::CamoOrigin::Expendable,
            });
        }
        result
    }

    /// Re-parse the PrototypeDatabase from owned bytes.
    pub(super) fn db(&self) -> Result<PrototypeDatabase<'_>, Report> {
        Ok(assets_bin::parse_assets_bin(&self.assets_bin_bytes).context("Failed to parse assets.bin")?)
    }

    /// Find a Vehicle by model directory name.
    fn find_vehicle(&self, model_dir: &str) -> Result<&crate::game_params::types::Vehicle, Report> {
        self.metadata
            .params()
            .iter()
            .filter_map(|p| p.vehicle())
            .find(|v| v.model_path().map(|mp| mp.contains(model_dir)).unwrap_or(false))
            .ok_or_else(|| rootcause::report!("Ship '{}' not found in GameParams", model_dir))
    }

    /// Find the ship param's full name (e.g. "PJSB018_Yamato_1944") from model directory.
    fn find_ship_index(&self, model_dir: &str) -> Option<String> {
        self.metadata
            .params()
            .iter()
            .find(|p| p.vehicle().and_then(|v| v.model_path()).map(|mp| mp.contains(model_dir)).unwrap_or(false))
            .map(|p| p.name().to_string())
    }

    /// Scan paths_storage for .visual files in a directory matching the name.
    pub(super) fn find_visual_paths(
        &self,
        db: &PrototypeDatabase<'_>,
        self_id_index: &HashMap<u64, usize>,
        model_dir: &str,
    ) -> Vec<(String, String)> {
        let needle = format!("/{model_dir}/");
        let mut result = Vec::new();

        for (i, entry) in db.paths_storage.iter().enumerate() {
            if !entry.name.ends_with(".visual") {
                continue;
            }
            let full_path = db.reconstruct_path(i, self_id_index);
            if full_path.contains(&needle) {
                let sub_name = entry.name.strip_suffix(".visual").unwrap_or(&entry.name).to_string();
                result.push((sub_name, full_path));
            }
        }

        result.sort_by(|a, b| a.0.cmp(&b.0));
        result
    }

    /// Load and parse all sub-models from (name, full_path) pairs.
    pub(super) fn load_sub_models(
        &self,
        db: &PrototypeDatabase<'_>,
        self_id_index: &HashMap<u64, usize>,
        visual_paths: &[(String, String)],
    ) -> Result<Vec<OwnedSubModel>, Report> {
        let mut result = Vec::new();

        for (sub_name, _) in visual_paths {
            let visual_suffix = format!("{sub_name}.visual");
            let vis_data = match resolve_visual_data(db, &visual_suffix, self_id_index) {
                Ok(data) => data,
                Err(e) => {
                    eprintln!("Warning: skipping '{visual_suffix}': {e}");
                    continue;
                }
            };
            let vp = visual::parse_visual(vis_data).context("Failed to parse VisualPrototype")?;

            let geom_path_idx = self_id_index.get(&vp.merged_geometry_path_id).ok_or_else(|| {
                rootcause::report!(
                    "Could not resolve mergedGeometryPathId 0x{:016X} for {}",
                    vp.merged_geometry_path_id,
                    sub_name
                )
            })?;
            let geom_full_path = db.reconstruct_path(*geom_path_idx, self_id_index);

            let mut geom_bytes = Vec::new();
            self.vfs
                .join(&geom_full_path)
                .context("VFS path error")?
                .open_file()
                .context_with(|| format!("Could not open geometry: {geom_full_path}"))?
                .read_to_end(&mut geom_bytes)?;

            // Try loading the .splash file (same directory, same base name).
            let splash_bytes = if geom_full_path.ends_with(".geometry") {
                let splash_path = format!("{}.splash", &geom_full_path[..geom_full_path.len() - ".geometry".len()]);
                let mut buf = Vec::new();
                match self.vfs.join(&splash_path).and_then(|p| p.open_file()) {
                    Ok(mut f) => {
                        let _ = f.read_to_end(&mut buf);
                        Some(buf)
                    }
                    Err(_) => None,
                }
            } else {
                None
            };

            result.push(OwnedSubModel { name: sub_name.clone(), visual: vp, geom_bytes, splash_bytes });
        }

        Ok(result)
    }

    /// Resolve (and deduplicate) a misc model by name, loading and validating its
    /// geometry so downstream render/export paths cannot fail on it.
    ///
    /// Returns the index into `misc_models`, or `None` if the model failed to load or
    /// has unparseable geometry (warned and remembered so repeats are cheap).
    fn resolve_misc_model(
        &self,
        db: &PrototypeDatabase<'_>,
        self_id_index: &HashMap<u64, usize>,
        model_leaves: &ModelLeaves<'_>,
        misc_name: &str,
        node_name: &str,
        acc: &mut MiscModelSet,
    ) -> Option<usize> {
        if let Some(&idx) = acc.index.get(misc_name) {
            return Some(idx);
        }
        if acc.load_failed.contains(misc_name) {
            return None;
        }
        // The game finds a misc by its `.model`; a visual without one is not a misc.
        if model_leaves.position(misc_name).is_none() {
            acc.load_failed.insert(misc_name.to_string());
            return None;
        }
        // Misc models resolve by leaf `<miscName>.visual`, exactly like turret mounts;
        // the game's `<nation>/misc/<miscName>/` directory rule is not needed because the
        // leaf name is unique across the path store. Geometry is parsed here (not lazily
        // at export) so a model with malformed geometry is warn+skipped, keeping the
        // export/render paths panic-free.
        match self.load_single_turret(db, self_id_index, &format!("{misc_name}.model")) {
            Ok(smd) if geometry::parse_geometry(&smd.geom_bytes).is_ok() => {
                let idx = acc.models.len();
                acc.index.insert(misc_name.to_string(), idx);
                acc.models.push(smd);
                Some(idx)
            }
            Ok(_) => {
                eprintln!("Warning: misc '{misc_name}' (node {node_name}) has unparseable geometry; skipping");
                acc.load_failed.insert(misc_name.to_string());
                None
            }
            Err(e) => {
                eprintln!("Warning: could not load misc '{misc_name}' (node {node_name}): {e}");
                acc.load_failed.insert(misc_name.to_string());
                None
            }
        }
    }

    /// Collect misc-part placements (`MP_` nodes) from the hull and from mounted models.
    ///
    /// Hull miscs (from the hull section skeleton extenders) all show: the game's
    /// `HullMiscsController` applies no `miscFilter`. Mount miscs come from each mounted
    /// model's own skeleton extenders and are gated by that mount's `miscFilter`
    /// whitelist plus the `battle`-preset `customMiscs`, matching the base
    /// `MiscsController`. Models are deduplicated across all placements; placements are
    /// never deduplicated (the same node name recurs with different transforms). Returns
    /// `(unique_misc_models, placements)`.
    fn collect_miscs(
        &self,
        db: &PrototypeDatabase<'_>,
        self_id_index: &HashMap<u64, usize>,
        model_leaves: &ModelLeaves<'_>,
        hull: &HullSections<'_>,
        mounts: &[ResolvedMount],
        turret_models: &[OwnedSubModel],
    ) -> (Vec<OwnedSubModel>, Vec<ResolvedMisc>) {
        let mut acc = MiscModelSet::default();
        let mut placements = Vec::new();

        // Hull miscs: every MP_ node shows (HullMiscsController applies no filter).
        // Zip the three index-aligned arrays so a corrupt record with mismatched lengths
        // truncates to the shortest rather than panicking on `[i]`.
        let hull_extenders = hull
            .parts
            .iter()
            .zip(&hull.models)
            .zip(&hull.extenders)
            .filter_map(|((part, section), extenders)| Some((part, section.as_ref()?, extenders)));
        for (part, section, extenders) in hull_extenders {
            let nodes =
                extenders.iter().flat_map(|ext| ext.name_ids.iter().zip(&ext.parent_name_ids).zip(&ext.matrices));
            for ((&name_id, &parent_id), matrix) in nodes {
                let Some(node_name) = db.strings.get_string_by_id(name_id) else {
                    continue;
                };
                if !node_name.starts_with(MISC_NODE_PREFIX) {
                    continue;
                }
                let misc_name = misc_name_from_node(node_name);
                let resolved =
                    self.resolve_misc_model(db, self_id_index, model_leaves, &misc_name, node_name, &mut acc);
                let Some(model_idx) = resolved else {
                    continue;
                };
                let parent_name = db.strings.get_string_by_id(parent_id);
                let transform = compose_misc_transform(parent_name, &matrix.0, &part.visual, hull.parts, &db.strings);
                placements.push(ResolvedMisc {
                    node: HardpointName::new(node_name),
                    parent: MiscParent::HullSection(section.clone()),
                    misc_model_index: model_idx,
                    transform: Some(transform),
                });
            }
        }

        // Mount miscs: the mount model's own MP_ nodes, whitelisted by miscFilter or by
        // the battle-preset customMiscs. Skeleton extenders are loaded once per model.
        let mut turret_skel_exts: HashMap<usize, Vec<skeleton_extender::SkeletonExtender>> = HashMap::new();
        for mount in mounts {
            if !mount_has_misc_selection(&mount.misc_filter, &mount.custom_miscs) {
                continue;
            }
            let turret = &turret_models[mount.turret_model_index];
            let exts = turret_skel_exts
                .entry(mount.turret_model_index)
                .or_insert_with(|| model_leaves.extenders(db, &turret.name));

            // Snapshot the parsed extenders so `self` is free for resolve_misc_model.
            let nodes: Vec<(String, u32, [f32; 16])> = exts
                .iter()
                .flat_map(|ext| ext.name_ids.iter().zip(&ext.parent_name_ids).zip(&ext.matrices))
                .filter_map(|((&name_id, &parent_id), matrix)| {
                    let node_name = db.strings.get_string_by_id(name_id)?;
                    node_name.starts_with(MISC_NODE_PREFIX).then(|| (node_name.to_string(), parent_id, matrix.0))
                })
                .collect();

            for (node_name, parent_id, local_matrix) in nodes {
                let misc_name = misc_name_from_node(&node_name);
                if !mount_shows_misc(&mount.misc_filter, &mount.custom_miscs, &node_name, &misc_name) {
                    continue;
                }
                let resolved =
                    self.resolve_misc_model(db, self_id_index, model_leaves, &misc_name, &node_name, &mut acc);
                let Some(model_idx) = resolved else {
                    continue;
                };
                // Position the misc in the turret model's own space (compose through its
                // parent chain, e.g. `Rotate_Y`), then attach at the raw hardpoint exactly
                // as the game does (`Model(..., node=parentModel.node(name))`). Use
                // `armor_transform` (the raw hardpoint transform), NOT `transform`: the
                // latter carries an `inv(Rotate_Y_BlendBone)` correction that un-reflects
                // the turret *geometry* (authored in the BlendBone frame). Misc nodes hang
                // off `Rotate_Y`, above that reflecting bone, so applying it would mirror
                // the misc model (an improper, det -1 transform).
                let parent_name = db.strings.get_string_by_id(parent_id);
                let local = compose_misc_transform(parent_name, &local_matrix, &turret.visual, &[], &db.strings);
                let transform = mat4_mul(&mount.armor_transform, &local);
                placements.push(ResolvedMisc {
                    node: HardpointName::new(node_name),
                    parent: MiscParent::Mount(HardpointName::new(mount.hp_name.as_str())),
                    misc_model_index: model_idx,
                    transform: Some(transform),
                });
            }
        }

        (acc.models, placements)
    }

    /// Load turret models and build mount resolution data.
    pub(super) fn load_mounts(
        &self,
        db: &PrototypeDatabase<'_>,
        self_id_index: &HashMap<u64, usize>,
        mount_points: &[MountPoint],
        hull_parts: &[OwnedSubModel],
    ) -> Result<LoadedMounts, Report> {
        let hp_transforms = hull_hardpoint_transforms(db, hull_parts);

        // Load unique turret models.
        let (turret_models, turret_model_index) = self.load_turret_models_deduped(db, self_id_index, mount_points)?;

        // Map hull HP names to turret model paths so we can find the parent
        // turret visual for compound hardpoints.
        let hp_to_model_path: HashMap<&str, &str> = mount_points
            .iter()
            .filter(|mi| !mi.model_path().is_empty() && hp_transforms.contains_key(mi.hp_name()))
            .map(|mi| (mi.hp_name(), mi.model_path()))
            .collect();

        // Build resolved mounts.
        let mut mounts = Vec::new();
        for mi in mount_points {
            let Some(&model_idx) = turret_model_index.get(mi.model_path()) else {
                continue;
            };

            // Resolve transform: simple HP from hull directly, compound HP
            // by composing parent (hull) and child (turret visual) transforms.
            // A mount is compound iff its HP name is NOT in the hull node tree.
            let (hull_transform, child_hp_transform) = if let Some(&xform) = hp_transforms.get(mi.hp_name()) {
                (xform, None)
            } else {
                match resolve_compound_hp(
                    mi.hp_name(),
                    &hp_transforms,
                    &hp_to_model_path,
                    &turret_model_index,
                    &turret_models,
                    &db.strings,
                ) {
                    Some(result) => result,
                    None => {
                        eprintln!("Warning: could not resolve hardpoint '{}'", mi.hp_name());
                        continue;
                    }
                }
            };
            let hp_transform = match child_hp_transform {
                None => hull_transform,
                Some(child_xform) => mat4_mul(&hull_transform, &child_xform),
            };

            // The turret model's Rotate_Y_BlendBone encodes its rest-pose
            // facing direction. To place the model correctly at the hardpoint
            // we undo this rest-pose rotation so the hardpoint's own orientation
            // takes over: visual_transform = hp_transform * inverse(bone_rotation).
            // Armor geometry is already aligned with the hardpoint, so it uses
            // the raw transform without rotation correction.
            let armor_transform = hp_transform;
            let turret_visual = &turret_models[model_idx].visual;
            let base_transform = match mesh_to_node_space(turret_visual, &db.strings) {
                Some(correction) => mat4_mul(&hp_transform, &correction),
                None => hp_transform,
            };

            let visual_transform = Some(base_transform);

            // Build per-vertex barrel pitch if a pitchDeadZone raises the barrels.
            let barrel_pitch = match mi.min_pitch_at_yaw(0.0) {
                Some(min_pitch) if min_pitch > 0.0 => build_barrel_pitch(turret_visual, &db.strings, min_pitch),
                _ => None,
            };

            mounts.push(ResolvedMount {
                hp_name: mi.hp_name().to_string(),
                turret_model_index: model_idx,
                transform: visual_transform,
                armor_transform,
                mount_armor: mi.mount_armor().cloned(),
                species: mi.species(),
                barrel_pitch,
                misc_filter: mi.misc_filter().to_vec(),
                custom_miscs: mi.custom_miscs().clone(),
            });
        }

        Ok(LoadedMounts { turret_models, turret_model_index, mounts })
    }

    /// Load unique turret models, deduplicating by model path.
    fn load_turret_models_deduped(
        &self,
        db: &PrototypeDatabase<'_>,
        self_id_index: &HashMap<u64, usize>,
        mount_points: &[MountPoint],
    ) -> Result<(Vec<OwnedSubModel>, HashMap<String, usize>), Report> {
        let mut index_map: HashMap<String, usize> = HashMap::new();
        let mut models = Vec::new();

        for mi in mount_points {
            if mi.model_path().is_empty() || index_map.contains_key(mi.model_path()) {
                continue;
            }

            match self.load_single_turret(db, self_id_index, mi.model_path()) {
                Ok(smd) => {
                    let idx = models.len();
                    index_map.insert(mi.model_path().to_string(), idx);
                    models.push(smd);
                }
                Err(e) => {
                    eprintln!("Warning: could not load turret '{}': {e}", mi.model_path());
                }
            }
        }

        Ok((models, index_map))
    }

    /// Load turret models (non-deduplicating variant for texture listing).
    fn load_turret_models(
        &self,
        db: &PrototypeDatabase<'_>,
        self_id_index: &HashMap<u64, usize>,
        mount_points: &[MountPoint],
    ) -> Result<Vec<OwnedSubModel>, Report> {
        let (models, _) = self.load_turret_models_deduped(db, self_id_index, mount_points)?;
        Ok(models)
    }

    /// Load a single turret model from its .model path.
    fn load_single_turret(
        &self,
        db: &PrototypeDatabase<'_>,
        self_id_index: &HashMap<u64, usize>,
        model_path: &str,
    ) -> Result<OwnedSubModel, Report> {
        let visual_path = model_path.replace(".model", ".visual");
        let visual_suffix = visual_path.rsplit('/').next().unwrap_or(&visual_path).to_string();

        let vis_data = resolve_visual_data(db, &visual_suffix, self_id_index)?;
        let vp = visual::parse_visual(vis_data).context("Failed to parse turret visual")?;

        let geom_path_idx = self_id_index
            .get(&vp.merged_geometry_path_id)
            .ok_or_else(|| rootcause::report!("Could not resolve geometry for turret '{}'", visual_suffix))?;
        let geom_full_path = db.reconstruct_path(*geom_path_idx, self_id_index);

        let mut geom_bytes = Vec::new();
        self.vfs
            .join(&geom_full_path)
            .context("VFS path error")?
            .open_file()
            .context_with(|| format!("Could not open turret geometry: {geom_full_path}"))?
            .read_to_end(&mut geom_bytes)?;

        let model_short_name =
            model_path.rsplit('/').next().unwrap_or(model_path).strip_suffix(".model").unwrap_or(model_path);

        Ok(OwnedSubModel { name: model_short_name.to_string(), visual: vp, geom_bytes, splash_bytes: None })
    }
}

// ---------------------------------------------------------------------------
// ShipModelContext — fully-loaded ship, ready for export
// ---------------------------------------------------------------------------

/// A fully-loaded ship model. Owns all bytes and parsed visuals.
///
/// Created via [`ShipAssets::load_ship()`]. Call [`export_glb()`](Self::export_glb)
/// to write the model to a file or buffer.
pub struct ShipModelContext {
    vfs: VfsPath,
    /// Shared with the `ShipAssets` it was loaded from: cloning this is a
    /// refcount bump, not a ~170 MB memcpy, so a dialog holding a context
    /// alive costs nothing extra here.
    assets_bin_bytes: Arc<Vec<u8>>,
    hull_parts: Vec<OwnedSubModel>,
    turret_models: Vec<OwnedSubModel>,
    mounts: Vec<ResolvedMount>,
    /// Unique misc-part models (propellers, boats, deck fittings), deduplicated by name.
    misc_models: Vec<OwnedSubModel>,
    /// Misc-part placements: one per `MP_` skeleton node.
    miscs: Vec<ResolvedMisc>,
    /// Every hull section, mount and misc placement with its hardpoints.
    parts: Vec<ShipPart>,
    info: ShipInfo,
    options: ShipExportOptions,
    mat_camo_schemes: Vec<MatCamoScheme>,
    /// Armor thickness map from GameParams.  See [`ArmorMap`].
    armor_map: Option<ArmorMap>,
    /// Hit location zones from GameParams, keyed by zone name (e.g. "Citadel").
    hit_locations: Option<HashMap<String, crate::game_params::types::HitLocation>>,
}

/// The `HP_` nodes of the hull visuals, by name. A name in more than one
/// visual resolves to the last visual's node, as mounts are placed.
pub(super) fn hull_hardpoint_transforms(
    db: &PrototypeDatabase<'_>,
    hull_parts: &[OwnedSubModel],
) -> HashMap<String, [f32; 16]> {
    let mut hp_transforms: HashMap<String, [f32; 16]> = HashMap::new();
    for smd in hull_parts {
        for &name_id in &smd.visual.nodes.name_map_name_ids {
            if let Some(name) = db.strings.get_string_by_id(name_id)
                && name.starts_with("HP_")
                && let Some(xform) = smd.visual.find_hardpoint_transform(name, &db.strings)
            {
                hp_transforms.insert(name.to_string(), xform);
            }
        }
    }

    hp_transforms
}

/// Resolve a visual suffix to VisualPrototype record data.
///
/// If the suffix resolves to blob 1 (VisualPrototype), returns the data directly.
/// If it resolves to blob 3 (ModelPrototype), parses the ModelPrototype and follows
/// its `visual_resource_id` to look up the actual VisualPrototype.
fn resolve_visual_data<'a>(
    db: &'a PrototypeDatabase<'a>,
    visual_suffix: &str,
    self_id_index: &HashMap<u64, usize>,
) -> Result<&'a [u8], Report> {
    let (vis_location, _) = db
        .resolve_path(visual_suffix, self_id_index)
        .context_with(|| format!("Could not resolve visual: {visual_suffix}"))?;

    match vis_location.blob_index {
        1 => {
            // Direct VisualPrototype
            Ok(db
                .get_prototype_data(vis_location, visual::VISUAL_ITEM_SIZE)
                .context("Failed to get visual prototype data")?)
        }
        3 => {
            // ModelPrototype -- follow visualResourceId to the actual VisualPrototype
            let model_data = db
                .get_prototype_data(vis_location, model::MODEL_ITEM_SIZE)
                .context("Failed to get model prototype data")?;
            let mp = model::parse_model(model_data)
                .context_with(|| format!("Failed to parse ModelPrototype for {visual_suffix}"))?;

            if mp.visual_resource_id == 0 {
                bail!("ModelPrototype for '{}' has null visualResourceId", visual_suffix);
            }

            // Look up the visual resource by its selfId
            let r2p_value = db.lookup_r2p(mp.visual_resource_id).ok_or_else(|| {
                rootcause::report!(
                    "visualResourceId 0x{:016X} from ModelPrototype '{}' not found in r2p map",
                    mp.visual_resource_id,
                    visual_suffix
                )
            })?;
            let vis_loc = db.decode_r2p_value(r2p_value).context("Failed to decode r2p value for visual resource")?;

            if vis_loc.blob_index != 1 {
                bail!(
                    "ModelPrototype '{}' visualResourceId resolved to blob {} (expected 1)",
                    visual_suffix,
                    vis_loc.blob_index
                );
            }

            Ok(db
                .get_prototype_data(vis_loc, visual::VISUAL_ITEM_SIZE)
                .context("Failed to get visual prototype data via ModelPrototype")?)
        }
        other => {
            bail!("'{}' resolved to blob {} (expected 1=Visual or 3=Model)", visual_suffix, other);
        }
    }
}

/// Per-LoD mesh counts, armor totals, and reachable MFM stems for one unique
/// model (a hull part, or one entry of `turret_models`/`misc_models`), decoded
/// exactly once regardless of how many times that model is instanced.
struct ModelSummary {
    mesh_lods: Vec<size_estimate::MeshCounts>,
    armor: size_estimate::MeshCounts,
    stems: HashSet<String>,
}

impl ShipModelContext {
    /// Ship identity information.
    pub fn info(&self) -> &ShipInfo {
        &self.info
    }

    /// Hull part names (sub-model names).
    pub fn hull_part_names(&self) -> Vec<&str> {
        self.hull_parts.iter().map(|p| p.name.as_str()).collect()
    }

    /// Every hull section, then every mount, then every misc placement, with
    /// placements and hardpoints in ship space. Keys match
    /// [`ModelLibrary::list_ship_parts`] for the same hull and overrides.
    pub fn parts(&self) -> &[ShipPart] {
        &self.parts
    }

    /// Number of mounted components (turrets, AA, etc.).
    pub fn mount_count(&self) -> usize {
        self.mounts.len()
    }

    /// Number of unique turret/mount 3D models.
    pub fn unique_turret_count(&self) -> usize {
        self.turret_models.len()
    }

    /// Number of misc-part placements (propellers, boats, deck fittings).
    pub fn misc_count(&self) -> usize {
        self.miscs.len()
    }

    /// Number of unique misc-part 3D models.
    pub fn unique_misc_count(&self) -> usize {
        self.misc_models.len()
    }

    /// Armor thickness map from GameParams.  See [`ArmorMap`].
    pub fn armor_map(&self) -> Option<&ArmorMap> {
        self.armor_map.as_ref()
    }

    /// Raw geometry bytes for hull parts, for inspection.
    pub fn hull_geom_bytes(&self) -> Vec<&[u8]> {
        self.hull_parts.iter().map(|p| p.geom_bytes.as_slice()).collect()
    }

    /// Raw geometry bytes for unique turret/mount models.
    pub fn turret_geom_bytes(&self) -> Vec<&[u8]> {
        self.turret_models.iter().map(|p| p.geom_bytes.as_slice()).collect()
    }

    /// Names of unique turret/mount models.
    pub fn turret_model_names(&self) -> Vec<&str> {
        self.turret_models.iter().map(|p| p.name.as_str()).collect()
    }

    /// Number of LOD levels available for hull meshes.
    pub fn hull_lod_count(&self) -> usize {
        self.hull_parts.iter().map(|p| p.visual.lods.len()).max().unwrap_or(1)
    }

    /// Hit location zones from GameParams (e.g. "Citadel" → HitLocation).
    pub fn hit_locations(&self) -> Option<&HashMap<String, crate::game_params::types::HitLocation>> {
        self.hit_locations.as_ref()
    }

    /// Raw splash file bytes for hull parts (if available).
    pub fn hull_splash_bytes(&self) -> Option<&[u8]> {
        self.hull_parts.iter().find_map(|p| p.splash_bytes.as_deref())
    }

    /// Build interactive armor meshes with per-triangle metadata.
    ///
    /// Returns one [`InteractiveArmorMesh`] per armor model found in the hull
    /// geometry. Each mesh contains the renderable triangle soup plus
    /// [`ArmorTriangleInfo`](gltf_export::ArmorTriangleInfo) entries aligned
    /// 1:1 with triangles, so a viewer can look up material name, thickness,
    /// and zone on hover/click.
    pub fn interactive_armor_meshes(&self) -> Result<Vec<InteractiveArmorMesh>, Report> {
        let mut result = Vec::new();

        // Hull armor (already in world space).
        for part in &self.hull_parts {
            let geom = geometry::parse_geometry(&part.geom_bytes)
                .context("Failed to parse hull geometry for interactive armor")?;
            for armor_model in &geom.armor_models {
                result.push(InteractiveArmorMesh::from_armor_model(armor_model, self.armor_map.as_ref(), None));
            }
        }

        // Turret armor: instance per mount.
        let turret_geoms: Vec<_> = self
            .turret_models
            .iter()
            .map(|part| {
                geometry::parse_geometry(&part.geom_bytes)
                    .context("Failed to parse turret geometry for interactive armor")
            })
            .collect::<Result<_, _>>()?;

        for mount in &self.mounts {
            let geom = &turret_geoms[mount.turret_model_index];
            for armor_model in &geom.armor_models {
                let mesh = InteractiveArmorMesh::from_armor_model(
                    armor_model,
                    self.armor_map.as_ref(),
                    mount.mount_armor.as_ref(),
                );
                result.push(mesh.placed_at_mount(&mount.hp_name, mount.armor_transform));
            }
        }

        Ok(result)
    }
    /// Collect hull visual meshes for interactive display.
    ///
    /// Returns one [`InteractiveHullMesh`](gltf_export::InteractiveHullMesh) per
    /// render set (hull parts + mounted turrets). LOD 0 is used.
    /// Base albedo textures are baked into per-vertex colors when available.
    pub fn interactive_hull_meshes(&self) -> Result<Vec<gltf_export::InteractiveHullMesh>, Report> {
        use std::io::Cursor;

        let db = assets_bin::parse_assets_bin(&self.assets_bin_bytes)
            .context("Failed to parse assets.bin for hull meshes")?;

        let lod = self.options.lod;
        let damaged = self.options.damaged;
        let mut result = Vec::new();

        // Hull parts (no transform, already in world space).
        for part in &self.hull_parts {
            let geom =
                geometry::parse_geometry(&part.geom_bytes).context("Failed to parse hull geometry for hull meshes")?;
            let meshes = gltf_export::collect_hull_meshes(&part.visual, &geom, &db, lod, damaged, None)?;
            result.extend(meshes);
        }

        // Mounted turrets (with mount transforms).
        for mount in &self.mounts {
            let part = &self.turret_models[mount.turret_model_index];
            let geom = geometry::parse_geometry(&part.geom_bytes)
                .context("Failed to parse turret geometry for hull meshes")?;
            let mut meshes =
                gltf_export::collect_hull_meshes(&part.visual, &geom, &db, lod, damaged, mount.barrel_pitch.as_ref())?;
            let group = PartGroup::from_mount_species(mount.species);
            for mesh in &mut meshes {
                mesh.transform = mount.transform.map(gltf_export::negate_z_transform);
                mesh.name = format!("{} [{}]", mesh.name, mount.hp_name);
                mesh.group = group;
            }
            result.extend(meshes);
        }

        // Misc parts (propellers, boats, deck fittings), instanced per placement.
        let misc_geoms: Vec<_> = self
            .misc_models
            .iter()
            .map(|part| {
                geometry::parse_geometry(&part.geom_bytes).context("Failed to parse misc geometry for hull meshes")
            })
            .collect::<Result<_, _>>()?;

        for misc in &self.miscs {
            let part = &self.misc_models[misc.misc_model_index];
            let geom = &misc_geoms[misc.misc_model_index];
            let mut meshes = gltf_export::collect_hull_meshes(&part.visual, geom, &db, lod, damaged, None)?;
            for mesh in &mut meshes {
                mesh.transform = misc.transform.map(gltf_export::negate_z_transform);
                mesh.name = format!("{} [{}]", mesh.name, misc.label());
                mesh.group = PartGroup::Misc;
            }
            result.extend(meshes);
        }

        // Bake base albedo textures into per-vertex colors.
        // Cache decoded images by MFM path to avoid re-loading the same texture.
        let mut texture_cache: HashMap<String, Option<image_dds::image::RgbaImage>> = HashMap::new();

        for mesh in &mut result {
            let mfm_path = match &mesh.mfm_path {
                Some(p) => p.clone(),
                None => continue,
            };
            if mesh.uvs.len() != mesh.positions.len() {
                continue;
            }

            let image = texture_cache.entry(mfm_path.clone()).or_insert_with(|| {
                let dds_bytes = texture::load_base_albedo_bytes(&self.vfs, &mfm_path, self.options.texture_lod)?;
                let dds = image_dds::ddsfile::Dds::read(&mut Cursor::new(&dds_bytes)).ok()?;
                image_dds::image_from_dds(&dds, 0).ok()
            });

            if let Some(img) = image {
                let width = img.width();
                let height = img.height();
                if width == 0 || height == 0 {
                    continue;
                }

                let mut colors = Vec::with_capacity(mesh.uvs.len());
                for uv in &mesh.uvs {
                    // Wrap UVs into [0, 1) range and sample the image.
                    let u = uv[0].rem_euclid(1.0);
                    let v = uv[1].rem_euclid(1.0);
                    let x = ((u * width as f32) as u32).min(width - 1);
                    let y = ((v * height as f32) as u32).min(height - 1);
                    let pixel = img.get_pixel(x, y);
                    colors.push([
                        pixel[0] as f32 / 255.0,
                        pixel[1] as f32 / 255.0,
                        pixel[2] as f32 / 255.0,
                        1.0, // alpha will be set by the viewer
                    ]);
                }
                mesh.colors = colors;
            }
        }

        Ok(result)
    }

    /// Base albedos for the given hull meshes, keyed by mfm_path, with the
    /// alpha their materials draw. Bakes TILEDLAND (underwater) materials, so
    /// the underwater hull gets its stock skin.
    pub fn hull_base_albedos(&self, meshes: &[gltf_export::InteractiveHullMesh]) -> HashMap<String, MaterialAlbedo> {
        let mut out = HashMap::new();
        let db = match assets_bin::parse_assets_bin(&self.assets_bin_bytes) {
            Ok(db) => db,
            Err(_) => return out,
        };
        let self_id_index = db.build_self_id_index();
        for mesh in meshes {
            let Some(path) = &mesh.mfm_path else { continue };
            if out.contains_key(path) {
                continue;
            }
            if let Some(albedo) = texture::load_or_bake_albedo(
                &self.vfs,
                path,
                mesh.mfm_path_id,
                Some(&db),
                Some(&self_id_index),
                self.options.texture_lod,
            ) {
                out.insert(path.clone(), albedo);
            }
        }
        out
    }

    /// Export the loaded ship model to GLB format.
    pub fn export_glb(&self, writer: &mut impl Write) -> Result<(), Report> {
        let db = assets_bin::parse_assets_bin(&self.assets_bin_bytes).context("Failed to re-parse assets.bin")?;

        // Parse geometries (scoped borrows — no self-referential issue).
        let hull_geoms: Vec<geometry::MergedGeometry<'_>> = self
            .hull_parts
            .iter()
            .map(|d| geometry::parse_geometry(&d.geom_bytes).expect("Failed to parse geometry"))
            .collect();

        let turret_geoms: Vec<geometry::MergedGeometry<'_>> = self
            .turret_models
            .iter()
            .map(|d| geometry::parse_geometry(&d.geom_bytes).expect("Failed to parse turret geometry"))
            .collect();

        let misc_geoms: Vec<geometry::MergedGeometry<'_>> = self
            .misc_models
            .iter()
            .map(|d| geometry::parse_geometry(&d.geom_bytes).expect("Failed to parse misc geometry"))
            .collect();

        // Build SubModel list.
        let mut sub_models: Vec<SubModel<'_>> = Vec::new();

        if self.options.contents.includes_mesh() {
            // Hull sub-models.
            for (data, geom) in self.hull_parts.iter().zip(hull_geoms.iter()) {
                sub_models.push(SubModel {
                    name: data.name.clone(),
                    visual: &data.visual,
                    geometry: geom,
                    transform: None,
                    group: PartGroup::Hull,
                    barrel_pitch: None,
                });
            }

            // Mounted components.
            for mount in &self.mounts {
                let turret_data = &self.turret_models[mount.turret_model_index];
                let turret_geom = &turret_geoms[mount.turret_model_index];

                sub_models.push(SubModel {
                    name: format!("{} ({})", mount.hp_name, turret_data.name),
                    visual: &turret_data.visual,
                    geometry: turret_geom,
                    transform: mount.transform,
                    group: PartGroup::from_mount_species(mount.species),
                    barrel_pitch: mount.barrel_pitch.clone(),
                });
            }

            // Misc parts (propellers, boats, deck fittings), instanced per placement.
            for misc in &self.miscs {
                let misc_data = &self.misc_models[misc.misc_model_index];
                let misc_geom = &misc_geoms[misc.misc_model_index];

                sub_models.push(SubModel {
                    name: format!("{} ({})", misc.label(), misc_data.name),
                    visual: &misc_data.visual,
                    geometry: misc_geom,
                    transform: misc.transform,
                    group: PartGroup::Misc,
                    barrel_pitch: None,
                });
            }
        }

        // Armor is untextured, so an armor-only export never reads a DDS.
        let texture_set = if self.options.textures && self.options.contents.includes_mesh() {
            self.selected_texture_set(&db)?
        } else {
            TextureSet::empty()
        };

        // Collect armor meshes from hull AND turret geometries with thickness data.
        let armor_map = self.armor_map.as_ref();
        let mut armor_meshes: Vec<gltf_export::ArmorSubModel> = Vec::new();
        if self.options.contents.includes_armor() {
            // Hull armor (already in world space, no transform needed).
            for geom in &hull_geoms {
                for am in &geom.armor_models {
                    armor_meshes.extend(gltf_export::armor_sub_models_by_zone(am, armor_map, None));
                }
            }

            // Turret armor: instance per mount with that mount's transform.
            for mount in &self.mounts {
                let turret_geom = &turret_geoms[mount.turret_model_index];
                for am in &turret_geom.armor_models {
                    let mut subs = gltf_export::armor_sub_models_by_zone(am, armor_map, mount.mount_armor.as_ref());
                    for s in &mut subs {
                        s.transform = Some(mount.armor_transform);
                        s.name = format!("{} [{}]", s.name, mount.hp_name);
                    }
                    armor_meshes.extend(subs);
                }
            }
        }

        let hardpoint_sets: Vec<HardpointSet<'_>> = if self.options.hardpoints {
            self.parts
                .iter()
                .map(|part| HardpointSet { name: part.key.label(), hardpoints: &part.hardpoints })
                .collect()
        } else {
            Vec::new()
        };

        gltf_export::export_ship_glb(
            &sub_models,
            &armor_meshes,
            &db,
            self.options.contents,
            self.options.lod,
            &texture_set,
            self.options.damaged,
            &hardpoint_sets,
            writer,
        )
        .context("Failed to export ship GLB")?;

        Ok(())
    }

    /// Derive everything pricing needs from one unique model's geometry
    /// metadata: per-LoD mesh counts, armor totals, and the MFM stems at
    /// least one surviving primitive carries. `gltf_export::
    /// primitive_summary_from_metadata` reads `items_count` straight out of
    /// the mapping metadata instead of meshopt-decoding every vertex/index
    /// buffer just to take `.len()`; it is proven to count identically to
    /// the decode-based path by
    /// `tests::metadata_counting_matches_decode_based_counting`.
    fn model_summary(
        &self,
        part: &OwnedSubModel,
        db: &PrototypeDatabase<'_>,
        self_id_index: &HashMap<u64, usize>,
    ) -> Result<ModelSummary, Report> {
        let geom = geometry::parse_geometry(&part.geom_bytes).context("Failed to parse geometry")?;

        let mut mesh_lods = Vec::with_capacity(part.visual.lods.len());
        let mut stems = HashSet::new();
        for lod in &part.visual.lods {
            let summary = gltf_export::primitive_summary_from_metadata(
                &part.visual,
                &geom,
                db,
                self_id_index,
                lod,
                self.options.damaged,
            )?;
            mesh_lods.push(summary.counts);
            stems.extend(summary.mfm_stems);
        }

        let mut armor = size_estimate::MeshCounts { vertices: 0, indices: 0 };
        for am in &geom.armor_models {
            for sub in gltf_export::armor_sub_models_by_zone(am, self.armor_map.as_ref(), None) {
                armor.vertices += sub.positions.len() as u64;
                armor.indices += sub.indices.len() as u64;
            }
        }

        Ok(ModelSummary { mesh_lods, armor, stems })
    }

    /// Gather everything that prices an export, once. The caller can then price
    /// any option combination without re-reading geometry or textures.
    ///
    /// Each unique hull part / turret model / misc model is decoded exactly
    /// once via [`Self::model_summary`] (Smaland: 63 unique models, not the
    /// 175 mount/misc instances that reference them), then indexed per
    /// instance to build `mesh_lods` and the ship-wide armor total, mirroring
    /// `export_glb`'s own instancing (every hull part once, one entry per
    /// mount through `turret_model_index`, one entry per misc placement
    /// through `misc_model_index`, with miscs contributing no armor) without
    /// redoing the decode per instance.
    pub fn size_model(&self) -> Result<size_estimate::ExportSizeModel, Report> {
        let db = assets_bin::parse_assets_bin(&self.assets_bin_bytes).context("Failed to parse assets.bin")?;
        let self_id_index = db.build_self_id_index();

        let hull_summaries: Vec<ModelSummary> =
            self.hull_parts.iter().map(|p| self.model_summary(p, &db, &self_id_index)).collect::<Result<_, _>>()?;
        let turret_summaries: Vec<ModelSummary> =
            self.turret_models.iter().map(|p| self.model_summary(p, &db, &self_id_index)).collect::<Result<_, _>>()?;
        let misc_summaries: Vec<ModelSummary> =
            self.misc_models.iter().map(|p| self.model_summary(p, &db, &self_id_index)).collect::<Result<_, _>>()?;

        let mut mesh_lods = Vec::new();
        let mut armor = size_estimate::MeshCounts { vertices: 0, indices: 0 };

        // Hull parts: already one entry each, no instancing.
        for summary in &hull_summaries {
            mesh_lods.push(summary.mesh_lods.clone());
            armor.vertices += summary.armor.vertices;
            armor.indices += summary.armor.indices;
        }
        // Mounted turrets: one entry per mount instance. Two mounts sharing a
        // turret model still count twice, since the export emits two meshes
        // (and two armor copies) for them.
        for mount in &self.mounts {
            let summary = &turret_summaries[mount.turret_model_index];
            mesh_lods.push(summary.mesh_lods.clone());
            armor.vertices += summary.armor.vertices;
            armor.indices += summary.armor.indices;
        }
        // Misc placements: one entry per placement instance. Miscs contribute
        // no armor to the export (export_glb's armor collection covers hulls
        // and mounts only), so their armor is never added.
        for misc in &self.miscs {
            mesh_lods.push(misc_summaries[misc.misc_model_index].mesh_lods.clone());
        }

        // A stem whose only render set is excluded (e.g. `_crack_` in an
        // intact export) has no reachable primitive anywhere, and pricing its
        // texture would charge for one the export never embeds.
        let mut reachable = HashSet::new();
        for summary in hull_summaries.iter().chain(turret_summaries.iter()).chain(misc_summaries.iter()) {
            reachable.extend(summary.stems.iter().cloned());
        }

        // Reuses the db already parsed above instead of paying for a second
        // assets.bin parse just to enumerate camo schemes.
        let source = self.camo_texture_source_from_db(&db)?;
        // A texture whose ladder cannot be read (missing/corrupt tail) contributes
        // zero rather than failing the whole model: this is a best-effort price, and
        // one bad texture must not deny the user a number.
        let ladder_for = |path: &str| texture::texture_ladder(&self.vfs, path).map(size_estimate::TextureLadder::new);

        // Filtered to reachable stems, then keyed by resolved path rather than
        // by stem: a "_wire"-suffixed material stem falls back to its
        // unsuffixed sibling's albedo (and other stems can coincide too), and
        // `ImageCache` embeds that shared file once. A stem-keyed map would
        // price every stem that shares a texture as its own copy and badly
        // overcount.
        let base_ladders: HashMap<String, size_estimate::TextureLadder> = source
            .base_albedo_paths()
            .into_iter()
            .filter(|(stem, _)| reachable.contains(stem))
            .filter_map(|(_, path)| ladder_for(&path).map(|l| (path, l)))
            .collect();

        // Only the schemes this ship's own options select, not every scheme the
        // ship offers (roughly 100 on Yamato): each one costs a VFS read of the
        // DDS tail plus up to three existence probes, and the dialog that drives
        // this only ever wants the ids the user has actually ticked. A scheme
        // selected later (a fresh camo pick) is priced lazily via
        // `Self::scheme_ladders_batch` and merged into the model with
        // `ExportSizeModel::insert_scheme_ladders`.
        let mut scheme_ladders: HashMap<CamoSchemeId, Vec<(String, size_estimate::TextureLadder)>> = HashMap::new();
        for id in self.options.camos.priced_scheme_ids() {
            let ladders: Vec<(String, size_estimate::TextureLadder)> = source
                .scheme_texture_paths(id, &reachable)?
                .into_iter()
                .filter_map(|path| ladder_for(&path).map(|l| (path, l)))
                .collect();
            scheme_ladders.insert(id, ladders);
        }

        Ok(size_estimate::ExportSizeModel::new(mesh_lods, armor, base_ladders, scheme_ladders))
    }

    /// Resolve and read several camo schemes' texture ladders in one pass,
    /// without touching geometry. `size_model` only prices the schemes
    /// `self.options.camos` selected at load time; a scheme the user selects
    /// afterward is priced lazily here and merged into the model with
    /// [`size_estimate::ExportSizeModel::insert_scheme_ladders`].
    ///
    /// The assets.bin re-parse, the self-id index and the reachable-stem set
    /// (a `model_summary` pass over every unique hull/turret/misc model) are
    /// each paid once for the whole batch, not once per id: calling this once
    /// per id in a loop would pay that shared cost N times for no reason.
    ///
    /// Reads DDS tails over the VFS, so callers must run this off the UI
    /// thread.
    pub fn scheme_ladders_batch(
        &self,
        ids: &[CamoSchemeId],
    ) -> Result<Vec<(CamoSchemeId, size_estimate::SchemeLadders)>, Report> {
        let db = assets_bin::parse_assets_bin(&self.assets_bin_bytes).context("Failed to parse assets.bin")?;
        let self_id_index = db.build_self_id_index();

        let mut reachable = HashSet::new();
        for part in self.hull_parts.iter().chain(self.turret_models.iter()).chain(self.misc_models.iter()) {
            reachable.extend(self.model_summary(part, &db, &self_id_index)?.stems);
        }

        let source = self.camo_texture_source_from_db(&db)?;
        let ladder_for = |path: &str| texture::texture_ladder(&self.vfs, path).map(size_estimate::TextureLadder::new);

        ids.iter()
            .map(|&id| {
                let ladders: size_estimate::SchemeLadders = source
                    .scheme_texture_paths(id, &reachable)?
                    .into_iter()
                    .filter_map(|path| ladder_for(&path).map(|l| (path, l)))
                    .collect();
                Ok((id, ladders))
            })
            .collect()
    }

    /// Build a lazy [`camo_textures::CamoTextureSource`] for this ship: cheap scheme
    /// metadata enumeration plus on-demand single-scheme decode.
    pub fn camo_texture_source(&self) -> Result<camo_textures::CamoTextureSource, Report> {
        let db = assets_bin::parse_assets_bin(&self.assets_bin_bytes)
            .context("Failed to parse assets.bin for camo source")?;
        self.camo_texture_source_from_db(&db)
    }

    /// [`Self::camo_texture_source`], reusing a `PrototypeDatabase` the caller already
    /// parsed instead of paying for a second assets.bin parse.
    fn camo_texture_source_from_db(
        &self,
        db: &PrototypeDatabase<'_>,
    ) -> Result<camo_textures::CamoTextureSource, Report> {
        let mut all_mfm_infos = Vec::new();
        for d in &self.hull_parts {
            all_mfm_infos.extend(collect_mfm_info(&d.visual, db));
        }
        for mount in &self.mounts {
            let turret = &self.turret_models[mount.turret_model_index];
            all_mfm_infos.extend(collect_mfm_info(&turret.visual, db));
        }
        for misc in &self.misc_models {
            all_mfm_infos.extend(collect_mfm_info(&misc.visual, db));
        }
        let mut seen = HashSet::new();
        let unique_infos: Vec<MfmInfo> = all_mfm_infos.into_iter().filter(|i| seen.insert(i.stem.clone())).collect();

        let stems: Vec<String> = unique_infos.iter().map(|i| i.stem.clone()).collect();
        let camo_texture_paths: HashSet<String> =
            self.mat_camo_schemes.iter().flat_map(|s| s.textures.values().cloned()).collect();
        let legacy_schemes = texture::discover_texture_schemes(&self.vfs, &stems, &camo_texture_paths);
        let mat_schemes = self.mat_camo_schemes.iter().map(|s| s.to_owned_scheme()).collect();

        Ok(camo_textures::CamoTextureSource::new(
            self.vfs.clone(),
            unique_infos,
            legacy_schemes,
            mat_schemes,
            self.options.texture_lod,
        ))
    }

    /// Base albedo plus exactly the selected camo schemes, enumerated by the same
    /// source the picker reads so a `CamoSchemeId` means one thing everywhere.
    fn selected_texture_set(&self, db: &PrototypeDatabase<'_>) -> Result<TextureSet, Report> {
        let source = self.camo_texture_source_from_db(db)?;
        let mut tex_set = TextureSet::empty();
        tex_set.base = source.base_albedos();
        tex_set.base_alpha = source.base_alpha();

        if let CamoSelection::Baked(id) = &self.options.camos {
            let info =
                source.scheme_info(*id).ok_or_else(|| Report::new(camo_textures::CamoDecodeError::UnknownId(*id)))?;
            let scheme = source.decode(*id)?;
            let bases = decode_png_map(&tex_set.base);
            let applied = camo_composite::apply_scheme(&scheme, &info.uv_transforms, info.use_color_scheme, &bases);

            for (stem, app) in applied {
                let (image, uv) = match app {
                    camo_composite::CamoApplication::CompositeOverBase { texture, uv } => {
                        // No shader here, so resolve it now. A stem is only classified
                        // this way when it has a base, so an absent one cannot happen.
                        let Some(base) = bases.get(&stem) else {
                            continue;
                        };
                        (camo_composite::flatten_over_base(&texture, &uv, base), UvTransform::default())
                    }
                    camo_composite::CamoApplication::Replace { texture, uv } => (texture, uv),
                };
                tex_set.base.insert(stem.clone(), encode_png(&image)?);
                if uv.scale != [1.0, 1.0] || uv.offset != [0.0, 0.0] {
                    tex_set.base_uv_transforms.insert(stem, [uv.scale[0], uv.scale[1], uv.offset[0], uv.offset[1]]);
                }
            }
            // Stems the scheme does not cover keep their base albedo, which is what
            // the game does: an uncovered part reverts to stock.
            return Ok(tex_set);
        }

        let CamoSelection::Variants(ids) = &self.options.camos else {
            return Ok(tex_set);
        };

        for (slot, id) in ids.iter().enumerate() {
            let info =
                source.scheme_info(*id).ok_or_else(|| Report::new(camo_textures::CamoDecodeError::UnknownId(*id)))?;
            let textures = source.decode(*id)?;
            tex_set.camo_schemes.push((info.display_name.clone(), textures));
            for (stem, xform) in &info.uv_transforms {
                tex_set
                    .tiled_uv_transforms
                    .insert((slot, stem.clone()), [xform.scale[0], xform.scale[1], xform.offset[0], xform.offset[1]]);
            }
        }
        Ok(tex_set)
    }
}

/// Decode a stem-to-PNG map into raw RGBA. A stem whose PNG will not decode is
/// dropped: the compositor then treats it as having no base, which is the same
/// outcome as a part the game ships without one.
fn decode_png_map(pngs: &HashMap<String, Vec<u8>>) -> HashMap<String, camo_composite::RgbaImageData> {
    let mut out = HashMap::new();
    for (stem, png) in pngs {
        let Ok(img) = image::load_from_memory(png) else {
            eprintln!("  Warning: failed to decode base albedo for {stem}");
            continue;
        };
        let rgba = img.to_rgba8();
        let (width, height) = (rgba.width(), rgba.height());
        out.insert(stem.clone(), camo_composite::RgbaImageData { width, height, pixels: rgba.into_raw() });
    }
    out
}

fn encode_png(image: &camo_composite::RgbaImageData) -> Result<Vec<u8>, Report> {
    let buf = image::RgbaImage::from_raw(image.width, image.height, image.pixels.clone()).ok_or_else(|| {
        Report::new(gltf_export::ExportError::BakedCamoDimensions {
            width: image.width,
            height: image.height,
            pixels: image.pixels.len(),
        })
    })?;
    let mut out = std::io::Cursor::new(Vec::new());
    buf.write_to(&mut out, image::ImageFormat::Png).context("Failed to encode baked camo")?;
    Ok(out.into_inner())
}

// ---------------------------------------------------------------------------
// Internal types
// ---------------------------------------------------------------------------

/// Owns visual + geometry bytes for one sub-model (no lifetime parameters).
pub(super) struct OwnedSubModel {
    name: String,
    visual: VisualPrototype,
    pub(super) geom_bytes: Vec<u8>,
    /// Raw `.splash` file bytes (only present for base hull models).
    splash_bytes: Option<Vec<u8>>,
}

/// Result of [`ShipAssets::load_mounts`].
pub(super) struct LoadedMounts {
    pub(super) turret_models: Vec<OwnedSubModel>,
    pub(super) turret_model_index: HashMap<String, usize>,
    pub(super) mounts: Vec<ResolvedMount>,
}

/// A mount instance with resolved transform.
pub(super) struct ResolvedMount {
    pub(super) hp_name: String,
    pub(super) turret_model_index: usize,
    /// Visual transform with yaw correction.
    transform: Option<[f32; 16]>,
    /// Raw hardpoint transform without model rotation (for armor geometry).
    pub(super) armor_transform: [f32; 16],
    /// Per-mount armor map for turret shell surfaces (from `A_Artillery.HP_XXX.armor`).
    mount_armor: Option<crate::game_params::types::ArmorMap>,
    /// Mount species from GameParams `typeinfo.species`.
    species: Option<crate::game_params::types::MountSpecies>,
    /// Per-vertex barrel pitch configuration (if pitchDeadZones applies).
    barrel_pitch: Option<super::gltf_export::BarrelPitch>,
    /// Whitelist of misc node names on this mount's model that are visible.
    misc_filter: Vec<String>,
    /// Preset-keyed extra misc names for this mount (`battle`/`dock` -> names).
    custom_miscs: HashMap<String, Vec<String>>,
}

/// Accumulates the deduplicated set of misc models while collecting placements.
#[derive(Default)]
struct MiscModelSet {
    /// Unique misc models, indexed by [`ResolvedMisc::misc_model_index`].
    models: Vec<OwnedSubModel>,
    /// Misc name -> index into `models`.
    index: HashMap<String, usize>,
    /// Misc names that failed to load or parse (warned once, then skipped).
    load_failed: HashSet<String>,
}

/// A misc-part placement: one instance of a misc model at a skeleton `MP_` node.
struct ResolvedMisc {
    /// Skeleton node name, e.g. "MP_BM509_Liferaft_type_20_10ft.001".
    node: HardpointName,
    parent: MiscParent,
    /// Index into [`ShipModelContext::misc_models`].
    misc_model_index: usize,
    /// Ship-space transform (column-major 4x4).
    transform: Option<[f32; 16]>,
}

impl ResolvedMisc {
    /// The node name followed by its parent part, which tells apart the nodes
    /// hull sections and mounts repeat.
    fn label(&self) -> String {
        match &self.parent {
            MiscParent::Mount(mount) => format!("{} [{mount}]", self.node),
            MiscParent::HullSection(section) => format!("{} [{}]", self.node, section.stem()),
        }
    }
}

/// The hull parts of a ship with the `.model` each is keyed by and the
/// skeleton extenders the game attaches to it.
struct HullSections<'a> {
    parts: &'a [OwnedSubModel],
    /// Aligned with `parts`. `None` for a visual the store holds no model beside.
    models: Vec<Option<ModelPath>>,
    /// Aligned with `parts`: the model's own extenders, then those the hull
    /// component adds in battle.
    extenders: Vec<Vec<skeleton_extender::SkeletonExtender>>,
}

impl<'a> HullSections<'a> {
    /// `visual_paths` holds `(sub_name, visual path)` for every hull visual
    /// found; `model_dir` is the ship model's stem.
    fn new(
        parts: &'a [OwnedSubModel],
        visual_paths: &[(String, String)],
        db: &PrototypeDatabase<'_>,
        self_id_index: &HashMap<u64, usize>,
        model_leaves: &ModelLeaves<'_>,
        model_dir: &str,
        upgrade: Option<&HullUpgradeConfig>,
    ) -> Self {
        let paths: HashMap<&str, &str> =
            visual_paths.iter().map(|(name, path)| (name.as_str(), path.as_str())).collect();
        let models = parts
            .iter()
            .map(|part| {
                let model = sibling_model_path(paths.get(part.name.as_str())?)?;
                let stored = db.reconstruct_path(model_leaves.position(&part.name)?, self_id_index);

                (stored == model.as_str()).then_some(model)
            })
            .collect();
        let extenders = parts
            .iter()
            .map(|part| {
                let mut extenders = model_leaves.extenders(db, &part.name);
                extenders.extend(battle_section_extenders(db, self_id_index, upgrade, model_dir, &part.name));
                extenders
            })
            .collect();

        Self { parts, models, extenders }
    }
}

/// Compose a misc node's ship-space transform from its parent node's world transform
/// and its own local matrix.
///
/// The parent is referenced by name into the base model skeleton, looked up in `own`
/// (the visual whose extender names the node) first, as sections reuse node names,
/// then in `others`. When it resolves (e.g. the model root "Scene Root", which is
/// identity), the composed transform is `parent_world * local`. When no visual holds
/// it the local matrix is already ship-space, so it is used directly.
fn compose_misc_transform(
    parent_name: Option<&str>,
    local: &[f32; 16],
    own: &VisualPrototype,
    others: &[OwnedSubModel],
    strings: &assets_bin::StringsSection<'_>,
) -> [f32; 16] {
    let parent_world = parent_name.and_then(|name| {
        std::iter::once(own)
            .chain(others.iter().map(|part| &part.visual))
            .find_map(|visual| visual.find_hardpoint_transform(name, strings))
    });

    match parent_world {
        Some(parent_world) => mat4_mul(&parent_world, local),
        None => *local,
    }
}

/// What [`ShipModelContext::parts`] is built from, borrowed during the load.
struct ShipPartSources<'a> {
    hull: &'a HullSections<'a>,
    turret_models: &'a [OwnedSubModel],
    /// GameParams model path to index into `turret_models`.
    turret_model_index: &'a HashMap<String, usize>,
    mounts: &'a [ResolvedMount],
    misc_models: &'a [OwnedSubModel],
    miscs: &'a [ResolvedMisc],
}

impl ShipPartSources<'_> {
    fn parts(
        &self,
        db: &PrototypeDatabase<'_>,
        self_id_index: &HashMap<u64, usize>,
        model_leaves: &ModelLeaves<'_>,
    ) -> Vec<ShipPart> {
        let mut parts = Vec::with_capacity(self.hull.parts.len() + self.mounts.len() + self.miscs.len());

        let hull_visuals: Vec<&VisualPrototype> = self.hull.parts.iter().map(|part| &part.visual).collect();
        for ((part, model), extenders) in self.hull.parts.iter().zip(&self.hull.models).zip(&self.hull.extenders) {
            // A part is keyed by its model path; a visual path that names none cannot be keyed.
            let Some(model) = model else {
                continue;
            };
            let hardpoints = model_space_hardpoints(db, part, extenders, &hull_visuals);
            parts.push(ShipPart {
                key: ShipPartKey::HullSection(model.clone()),
                model: Some(model.clone()),
                group: PartGroup::Hull,
                placement: None,
                hardpoints: hardpoints.placed(None),
            });
        }

        let mut turret_paths: Vec<Option<ModelPath>> = vec![None; self.turret_models.len()];
        for (path, &idx) in self.turret_model_index {
            if let Some(slot) = turret_paths.get_mut(idx) {
                *slot = ModelPath::parse(path).ok();
            }
        }
        let turret_hardpoints: Vec<ModelSpaceHardpoints> = self
            .turret_models
            .iter()
            .map(|turret| model_space_hardpoints(db, turret, &model_leaves.extenders(db, &turret.name), &[]))
            .collect();
        for mount in self.mounts {
            let idx = mount.turret_model_index;
            parts.push(ShipPart {
                key: ShipPartKey::Mount(HardpointName::new(mount.hp_name.as_str())),
                model: turret_paths[idx].clone(),
                group: PartGroup::from_mount_species(mount.species),
                placement: Some(negate_z_transform(mount.armor_transform)),
                hardpoints: turret_hardpoints[idx].placed(Some(&mount.armor_transform)),
            });
        }

        let misc_paths: Vec<Option<ModelPath>> = self
            .misc_models
            .iter()
            .map(|misc| {
                let position = model_leaves.position(&misc.name)?;
                ModelPath::parse(&db.reconstruct_path(position, self_id_index)).ok()
            })
            .collect();
        let misc_hardpoints: Vec<ModelSpaceHardpoints> = self
            .misc_models
            .iter()
            .map(|misc| model_space_hardpoints(db, misc, &model_leaves.extenders(db, &misc.name), &[]))
            .collect();
        for misc in self.miscs {
            let idx = misc.misc_model_index;
            parts.push(ShipPart {
                key: ShipPartKey::Misc { node: misc.node.clone(), parent: misc.parent.clone() },
                model: misc_paths[idx].clone(),
                group: PartGroup::Misc,
                placement: misc.transform.map(negate_z_transform),
                hardpoints: misc_hardpoints[idx].placed(misc.transform.as_ref()),
            });
        }

        parts
    }
}

/// Every `.model` entry by stem, the first in store order winning, as the
/// loader's name lookups resolve them.
struct ModelLeaves<'d> {
    positions: HashMap<&'d str, usize>,
}

impl<'d> ModelLeaves<'d> {
    fn new(db: &'d PrototypeDatabase<'_>) -> Self {
        let mut positions = HashMap::new();
        for (position, entry) in db.paths_storage.iter().enumerate() {
            if let Some(stem) = entry.name.strip_suffix(".model") {
                positions.entry(stem).or_insert(position);
            }
        }

        Self { positions }
    }

    fn position(&self, stem: &str) -> Option<usize> {
        self.positions.get(stem).copied()
    }

    /// The prototype of the model named `stem`, when its record reads.
    fn prototype(&self, db: &PrototypeDatabase<'_>, stem: &str) -> Option<model::ModelPrototype> {
        const MODEL_BLOB_INDEX: usize = 3;

        let entry = db.paths_storage.get(self.position(stem)?)?;
        let location = db.decode_r2p_value(db.lookup_r2p(entry.self_id)?).ok()?;
        if location.blob_index != MODEL_BLOB_INDEX {
            return None;
        }
        let record = db.get_prototype_data(location, model::MODEL_ITEM_SIZE).ok()?;

        model::parse_model(record).ok()
    }

    /// The skeleton extenders the model named `stem` attaches, as the game
    /// attaches them; empty when it has none or its record does not read.
    fn extenders(&self, db: &PrototypeDatabase<'_>, stem: &str) -> Vec<skeleton_extender::SkeletonExtender> {
        self.prototype(db, stem).map(|model| model_extenders(db, &model)).unwrap_or_default()
    }
}

/// The `.model` beside a `.visual`.
fn sibling_model_path(visual_path: &str) -> Option<ModelPath> {
    let stem = visual_path.strip_suffix(".visual")?;

    ModelPath::parse(&format!("{stem}.model")).ok()
}

/// The `HP_` nodes of `part`'s visual and the `MP_`/`EP_` nodes of
/// `extenders`, whose parents are looked up in `part`'s own visual first, then
/// in `other_visuals`.
fn model_space_hardpoints(
    db: &PrototypeDatabase<'_>,
    part: &OwnedSubModel,
    extenders: &[skeleton_extender::SkeletonExtender],
    other_visuals: &[&VisualPrototype],
) -> ModelSpaceHardpoints {
    let mut hardpoints = ModelSpaceHardpoints::new();
    hardpoints.add_visual(&part.visual, &db.strings);
    hardpoints.add_extenders(extenders, &part.visual, other_visuals, &db.strings);

    hardpoints
}

/// Pre-resolved material-based camouflage scheme (owned data, no lifetimes).
struct MatCamoScheme {
    /// Display name for the variant (translated or fallback).
    display_name: String,
    /// Per-part albedo texture VFS paths (category to path), from camouflages.xml.
    textures: HashMap<String, String>,
    /// Whether this is a tiled camo (UV tiling via KHR_texture_transform).
    tiled: bool,
    /// Whether the camo recolors over the base albedo (preserving ship detail) vs pasting opaque.
    use_color_scheme: bool,
    /// Resolved color scheme colors when the camo references one (4 RGBA, linear).
    color_scheme_colors: Option<[[f32; 4]; 4]>,
    /// Per-part UV transforms for tiled camos. Key = part category (lowercase).
    uv_transforms: HashMap<String, camouflage::UvTransform>,
    /// The camouflages.xml camo name (for de-duplication across discovery sources).
    camo_name: String,
    /// Where this scheme came from (for UI grouping).
    origin: gltf_export::CamoOrigin,
}

impl MatCamoScheme {
    fn to_owned_scheme(&self) -> crate::export::camo_textures::OwnedMatScheme {
        crate::export::camo_textures::OwnedMatScheme {
            display_name: self.display_name.clone(),
            textures: self.textures.clone(),
            tiled: self.tiled,
            use_color_scheme: self.use_color_scheme,
            color_scheme_colors: self.color_scheme_colors,
            uv_transforms: self.uv_transforms.clone(),
            origin: self.origin,
            camo_name: self.camo_name.clone(),
        }
    }
}

// ---------------------------------------------------------------------------
// Shared helpers (pub so main.rs export-model can use them too)
// ---------------------------------------------------------------------------

/// Resolve the camo texture path for a stem's part category. `classify_part_category` returns
/// "tile" for hull stems, but an explicit hull texture is keyed "hull".
///
/// A tiled camo defines only a `tile` texture and is designed to cover the whole ship (with
/// per-category UV transforms), so it falls back to that tile for any part. A painted camo maps
/// each part category to its own albedo and covers only the categories it names; a part whose
/// category the camo does not define keeps its stock albedo (returns None), matching the game
/// (the whole-ship hull atlas has a different UV layout than fittings like glass or radars).
///
/// `glass` and `net` are never camouflaged in any mode: transparent glass is excluded from camo at
/// the engine level, and alpha-cutout nets/grids would lose their cutout under an opaque tile. They
/// keep their stock texture even under a whole-ship tiled camo (returns None).
pub(crate) fn resolve_part_texture<'a>(
    textures: &'a HashMap<String, String>,
    category: &str,
    tiled: bool,
) -> Option<&'a String> {
    if matches!(category, "glass" | "net") {
        return None;
    }
    let explicit_key = if category == "tile" { "hull" } else { category };
    let explicit = textures.get(explicit_key);
    if explicit.is_some() || !tiled {
        return explicit;
    }
    textures.get("tile").or_else(|| textures.get("hull"))
}

/// Resolved MFM info: stem (leaf name without `.mfm`), full VFS path, and
/// the alpha its material draws.
pub struct MfmInfo {
    pub stem: String,
    pub full_path: String,
    pub alpha: AlphaMode,
}

/// Collect MFM stems and full paths from a visual's render sets.
pub fn collect_mfm_info(visual: &VisualPrototype, db: &PrototypeDatabase<'_>) -> Vec<MfmInfo> {
    let self_id_index = db.build_self_id_index();
    let mut result = Vec::new();
    let mut seen = HashSet::new();

    for rs in &visual.render_sets {
        if rs.material_mfm_path_id == 0 {
            continue;
        }
        let Some(&path_idx) = self_id_index.get(&rs.material_mfm_path_id) else {
            continue;
        };
        let mfm_name = &db.paths_storage[path_idx].name;
        let stem = mfm_name.strip_suffix(".mfm").unwrap_or(mfm_name);

        if seen.insert(stem.to_string()) {
            let full_path = db.reconstruct_path(path_idx, &self_id_index);
            // Without the material record nothing marks the alpha as opacity.
            let alpha = texture::parse_mfm_from_db(db, rs.material_mfm_path_id)
                .map_or(AlphaMode::Opaque, |material| AlphaMode::of_material(&material, &full_path));
            result.push(MfmInfo { stem: stem.to_string(), full_path, alpha });
        }
    }

    result
}

/// Base albedo PNGs for a set of MFM infos. Camo schemes come from
/// `CamoTextureSource`, which is the single enumeration ids are defined against.
pub fn build_texture_set(mfm_infos: &[MfmInfo], vfs: &VfsPath, lod: texture::TextureLod) -> TextureSet {
    let mut base = HashMap::new();
    let mut base_alpha = HashMap::new();

    let mut seen_stems = HashSet::new();
    let mut unique_infos: Vec<&MfmInfo> = Vec::new();
    for info in mfm_infos {
        if seen_stems.insert(info.stem.clone()) {
            unique_infos.push(info);
        }
    }

    // Load base albedo textures.
    for info in &unique_infos {
        if let Some(dds_bytes) = texture::load_base_albedo_bytes(vfs, &info.full_path, lod) {
            match texture::dds_to_png(&dds_bytes, lod) {
                Ok(png_bytes) => {
                    base.insert(info.stem.clone(), png_bytes);
                    if info.alpha != AlphaMode::Opaque {
                        base_alpha.insert(info.stem.clone(), info.alpha);
                    }
                }
                Err(e) => {
                    eprintln!("  Warning: failed to decode base texture for {}: {e}", info.stem);
                }
            }
        }
    }

    TextureSet { base, base_alpha, ..TextureSet::empty() }
}

/// Resolve a compound hardpoint (e.g. `HP_AGM_3_HP_AGA_1`) by finding the
/// longest hull HP name that prefixes the mount's HP name, then looking up
/// the child HP in the parent turret's visual node tree.
///
/// Returns `(parent_hull_transform, Some(child_turret_transform))` on success.
fn resolve_compound_hp(
    hp_name: &str,
    hp_transforms: &HashMap<String, [f32; 16]>,
    hp_to_model_path: &HashMap<&str, &str>,
    turret_model_index: &HashMap<String, usize>,
    turret_models: &[OwnedSubModel],
    strings: &assets_bin::StringsSection<'_>,
) -> Option<([f32; 16], Option<[f32; 16]>)> {
    let (parent_hp, child_hp) = split_compound_hp(hp_name, hp_transforms.keys().map(String::as_str))?;
    let parent_xform = hp_transforms.get(parent_hp)?;

    // Find the parent turret model via the hp_to_model_path mapping.
    let &parent_model_path = hp_to_model_path.get(parent_hp)?;
    let &parent_turret_idx = turret_model_index.get(parent_model_path)?;
    let parent_turret = &turret_models[parent_turret_idx];

    // Look up the child HP transform in the parent turret's visual.
    let child_xform = parent_turret.visual.find_hardpoint_transform(child_hp, strings)?;

    Some((*parent_xform, Some(child_xform)))
}

/// Build a [`BarrelPitch`] config for per-vertex barrel rotation.
///
/// Finds the `Rotate_X` pivot point in turret-local space, builds a pitch
/// rotation matrix around it, and identifies which blend bone indices are
/// barrel bones (descendants of `Rotate_X` in the skeleton hierarchy).
fn build_barrel_pitch(
    visual: &VisualPrototype,
    strings: &assets_bin::StringsSection<'_>,
    min_pitch_deg: f32,
) -> Option<super::gltf_export::BarrelPitch> {
    let rotate_x_idx = visual.find_node_index_by_name("Rotate_X", strings)?;

    // Get the Rotate_X world (composed) transform to find pivot position.
    let rotate_x_world = visual.find_hardpoint_transform("Rotate_X", strings)?;
    let pivot = [rotate_x_world[12], rotate_x_world[13], rotate_x_world[14]];

    // Build pitch rotation matrix: T(-pivot) * Rx(-pitch) * T(pivot)
    let pitch_rad = -min_pitch_deg.to_radians();
    let (sin_p, cos_p) = pitch_rad.sin_cos();

    // Rx rotation (column-major):
    //   1     0      0
    //   0   cos_p  -sin_p
    //   0   sin_p   cos_p
    let pitch_matrix = [
        // col 0
        1.0,
        0.0,
        0.0,
        0.0,
        // col 1
        0.0,
        cos_p,
        sin_p,
        0.0,
        // col 2
        0.0,
        -sin_p,
        cos_p,
        0.0,
        // col 3: T(-pivot) * Rx * T(pivot) translation
        0.0,
        pivot[1] - cos_p * pivot[1] + sin_p * pivot[2],
        pivot[2] + sin_p * pivot[1] - cos_p * pivot[2],
        1.0,
    ];

    // Identify barrel bone indices from the first render set's blend bone list.
    // Barrel bones = those that ARE `Rotate_X` or descendants of it.
    let first_rs = visual.render_sets.first()?;
    let mut barrel_bone_indices = Vec::new();
    for (blend_idx, &name_id) in first_rs.node_name_ids.iter().enumerate() {
        // Resolve name_id → node index in skeleton
        let node_idx = visual
            .nodes
            .name_map_name_ids
            .iter()
            .position(|&nid| nid == name_id)
            .map(|i| visual.nodes.name_map_node_ids[i]);
        if let Some(ni) = node_idx
            && (ni == rotate_x_idx || visual.is_descendant_of(ni, rotate_x_idx))
        {
            barrel_bone_indices.push(blend_idx as u8);
        }
    }

    if barrel_bone_indices.is_empty() {
        return None;
    }

    // Conjugate the pitch matrix for Z-negated coordinate space (left→right-handed).
    let pitch_matrix = super::gltf_export::negate_z_transform(pitch_matrix);

    Some(super::gltf_export::BarrelPitch { pitch_matrix, barrel_bone_indices })
}

// ---------------------------------------------------------------------------
// Convenience function
// ---------------------------------------------------------------------------

/// One-shot: load game data, resolve ship, export GLB.
///
/// For multiple ships, use [`ShipAssets`] directly to amortize the ~18s
/// GameParams parsing cost.
pub fn export_ship_glb(
    vfs: &VfsPath,
    name: &str,
    options: &ShipExportOptions,
    writer: &mut impl Write,
) -> Result<ShipInfo, Report> {
    let assets = ShipAssets::load(vfs)?;
    let ctx = assets.load_ship(name, options)?;
    let info = ctx.info().clone();
    ctx.export_glb(writer)?;
    Ok(info)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole correctness argument for skipping the meshopt decode:
    /// `gltf_export::primitive_summary_from_metadata` (what `model_summary`
    /// calls) must count and reach exactly the same MFM stems as
    /// `gltf_export::primitive_summary` (the decode-based path it replaced),
    /// for every unique model at every LoD, under both damage states. Damage
    /// state matters because it changes which exclusion list
    /// (`INTACT_EXCLUDE`/`DAMAGED_EXCLUDE`) applies, and this test also
    /// confirms the filter is not a no-op it happens to agree on: it counts
    /// how many render sets each pass actually excludes and requires at
    /// least one under `damaged = true`, where `_hide` render sets are common
    /// on ship hulls. Ignored: needs a World of Warships install.
    #[test]
    #[ignore = "requires a World of Warships install"]
    fn metadata_counting_matches_decode_based_counting() {
        let game_dir = std::env::var_os("WOWS_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from(r"E:\WoWs\World_of_Warships"));
        let assets = ShipAssets::from_game_dir(&game_dir).expect("assets");
        let vehicle = assets
            .metadata()
            .params()
            .iter()
            .filter_map(|p| p.vehicle())
            .find(|v| v.model_path().map(|mp| mp.contains("WSD011_Smaland_1955")).unwrap_or(false))
            .cloned()
            .expect("Smaland vehicle");
        let ctx = assets.load_ship_from_vehicle(&vehicle, &ShipExportOptions::default()).expect("ctx");

        let db = assets_bin::parse_assets_bin(&ctx.assets_bin_bytes).expect("parse assets.bin");
        let self_id_index = db.build_self_id_index();

        let mut compared = 0usize;
        for damaged in [false, true] {
            let exclude_list = if damaged { gltf_export::DAMAGED_EXCLUDE } else { gltf_export::INTACT_EXCLUDE };
            let mut excluded_count = 0usize;

            for part in ctx.hull_parts.iter().chain(ctx.turret_models.iter()).chain(ctx.misc_models.iter()) {
                let geom = geometry::parse_geometry(&part.geom_bytes).expect("geometry");
                for (lod_idx, lod) in part.visual.lods.iter().enumerate() {
                    for &rs_name_id in &lod.render_set_names {
                        if let Some(rs_name) = db.strings.get_string_by_id(rs_name_id)
                            && exclude_list.iter().any(|sub| rs_name.contains(sub))
                        {
                            excluded_count += 1;
                        }
                    }

                    let decoded =
                        gltf_export::primitive_summary(&part.visual, &geom, &db, &self_id_index, lod, damaged)
                            .unwrap_or_else(|e| {
                                panic!(
                                    "decode-based summary failed for {} lod {lod_idx} damaged={damaged}: {e}",
                                    part.name
                                )
                            });
                    let metadata = gltf_export::primitive_summary_from_metadata(
                        &part.visual,
                        &geom,
                        &db,
                        &self_id_index,
                        lod,
                        damaged,
                    )
                    .unwrap_or_else(|e| {
                        panic!("metadata summary failed for {} lod {lod_idx} damaged={damaged}: {e}", part.name)
                    });

                    assert_eq!(
                        decoded.counts, metadata.counts,
                        "{} lod {lod_idx} damaged={damaged}: metadata counting diverged from decode-based counting",
                        part.name
                    );
                    assert_eq!(
                        decoded.mfm_stems, metadata.mfm_stems,
                        "{} lod {lod_idx} damaged={damaged}: reachable stems diverged between the two paths",
                        part.name
                    );
                    compared += 1;
                }
            }

            eprintln!("damaged={damaged}: {excluded_count} render sets excluded");
            if damaged {
                assert!(
                    excluded_count > 0,
                    "expected at least one _hide/_patch_ render set to be excluded under damaged=true on Smaland;                      an always-zero exclusion count would mean the filter equivalence is untested"
                );
            }
        }
        assert!(compared > 50, "expected many (part, lod) pairs on Smaland, got {compared}");
    }

    /// The lazy scheme-pricing path the dialog calls in the background after
    /// the user selects a camo `size_model` was not built with: `size_model`
    /// itself (via `CamoSelection::priced_scheme_ids`) prices zero schemes for
    /// `ShipExportOptions::default()` (`CamoSelection::BaseOnly`), so every
    /// scheme is initially "pending"; `scheme_ladders_batch` must resolve one
    /// on its own and produce ladders `ExportSizeModel::insert_scheme_ladders`
    /// can merge in to clear that. Ignored: needs a World of Warships install.
    #[test]
    #[ignore = "requires a World of Warships install"]
    fn scheme_ladders_prices_a_scheme_the_size_model_was_not_built_with() {
        let game_dir = std::env::var_os("WOWS_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from(r"E:\WoWs\World_of_Warships"));
        let assets = ShipAssets::from_game_dir(&game_dir).expect("assets");
        let vehicle = assets
            .metadata()
            .params()
            .iter()
            .filter_map(|p| p.vehicle())
            .find(|v| v.model_path().map(|mp| mp.contains("WSD011_Smaland_1955")).unwrap_or(false))
            .cloned()
            .expect("Smaland vehicle");
        let ctx = assets.load_ship_from_vehicle(&vehicle, &ShipExportOptions::default()).expect("ctx");

        let mut model = ctx.size_model().expect("size model");
        let source = ctx.camo_texture_source().expect("camo source");
        let id = source.scheme_infos().first().expect("Smaland has at least one scheme").id;

        let options =
            ShipExportOptions { camos: CamoSelection::Variants(vec![id]), textures: true, ..Default::default() };
        assert_eq!(
            model.unpriced_scheme_ladders(&options),
            vec![(id, size_estimate::SchemeLadderStatus::Pending)],
            "size_model built from BaseOnly options prices no schemes up front"
        );
        assert!(model.priced_estimate(&options).is_err(), "a pending scheme must refuse to price silently");

        let ladders = ctx
            .scheme_ladders_batch(&[id])
            .expect("scheme_ladders_batch")
            .into_iter()
            .next()
            .map(|(_, ladders)| ladders)
            .expect("one id requested, one result returned");
        assert!(!ladders.is_empty(), "Smaland's first scheme should resolve at least one texture path");

        model.insert_scheme_ladders(id, ladders);
        assert!(model.unpriced_scheme_ladders(&options).is_empty(), "inserting the fetched ladders clears the gap");
        assert!(model.priced_estimate(&options).unwrap().texture_bytes > 0, "the newly priced scheme must add bytes");
    }

    /// `scheme_ladders_batch` hoists the assets.bin re-parse, self-id index
    /// and reachable-stem pass out of the per-id loop that calling it once
    /// per id would otherwise pay N times. Measures both approaches for
    /// three schemes on Smaland and reports the wall-clock, since a batch API
    /// that isn't actually cheaper would be pointless ceremony. Ignored:
    /// needs a World of Warships install.
    #[test]
    #[ignore = "requires a World of Warships install"]
    fn scheme_ladders_batch_beats_looping_it_per_id() {
        let game_dir = std::env::var_os("WOWS_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from(r"E:\WoWs\World_of_Warships"));
        let assets = ShipAssets::from_game_dir(&game_dir).expect("assets");
        let vehicle = assets
            .metadata()
            .params()
            .iter()
            .filter_map(|p| p.vehicle())
            .find(|v| v.model_path().map(|mp| mp.contains("WSD011_Smaland_1955")).unwrap_or(false))
            .cloned()
            .expect("Smaland vehicle");
        let ctx = assets.load_ship_from_vehicle(&vehicle, &ShipExportOptions::default()).expect("ctx");

        let source = ctx.camo_texture_source().expect("camo source");
        let ids: Vec<CamoSchemeId> = source.scheme_infos().iter().take(3).map(|i| i.id).collect();
        assert_eq!(ids.len(), 3, "Smaland should offer at least three schemes");

        let looped_started = std::time::Instant::now();
        for &id in &ids {
            ctx.scheme_ladders_batch(&[id]).expect("scheme_ladders_batch");
        }
        let looped_elapsed = looped_started.elapsed();
        eprintln!("[measured] three schemes via scheme_ladders_batch() looped one id at a time: {looped_elapsed:?}");

        let batch_started = std::time::Instant::now();
        let batch = ctx.scheme_ladders_batch(&ids).expect("scheme_ladders_batch");
        let batch_elapsed = batch_started.elapsed();
        eprintln!("[measured] three schemes via scheme_ladders_batch() in one call: {batch_elapsed:?}");

        assert_eq!(batch.len(), 3);
        assert!(
            batch_elapsed < looped_elapsed,
            "batching should avoid paying the shared assets.bin/reachable-stem cost three times              (looped {looped_elapsed:?}, batch {batch_elapsed:?})"
        );
    }

    /// Finding 4's fix: `ShipModelContext::assets_bin_bytes` shares its
    /// `Arc<Vec<u8>>` with `ShipAssets` instead of cloning a fresh ~170 MB
    /// buffer per ship load. Loads the same ship several times over one
    /// `ShipAssets` and reports the average -- there is no "before" to
    /// compare against in this binary (the field type itself changed), so
    /// this is paired with a standalone `Vec`- vs `Arc`-clone microbenchmark
    /// at a realistic size to make the eliminated cost concrete. Ignored:
    /// needs a World of Warships install.
    #[test]
    #[ignore = "requires a World of Warships install"]
    fn repeated_ship_loads_do_not_pay_a_redundant_assets_bin_clone() {
        let game_dir = std::env::var_os("WOWS_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from(r"E:\WoWs\World_of_Warships"));
        let assets = ShipAssets::from_game_dir(&game_dir).expect("assets");
        let vehicle = assets
            .metadata()
            .params()
            .iter()
            .filter_map(|p| p.vehicle())
            .find(|v| v.model_path().map(|mp| mp.contains("WSD011_Smaland_1955")).unwrap_or(false))
            .cloned()
            .expect("Smaland vehicle");

        const ITERS: u32 = 3;
        let mut total = std::time::Duration::ZERO;
        for i in 0..ITERS {
            let started = std::time::Instant::now();
            let ctx = assets.load_ship_from_vehicle(&vehicle, &ShipExportOptions::default()).expect("ctx");
            let elapsed = started.elapsed();
            eprintln!("[measured] load_ship_from_vehicle call {i}: {elapsed:?}");
            total += elapsed;
            drop(ctx);
        }
        eprintln!("[measured] average over {ITERS} calls: {:?}", total / ITERS);

        // Standalone microbenchmark: what a Vec<u8> clone at
        // assets.bin's real size (170,699,420 bytes, per the review that
        // raised this finding) used to cost per call, against what an
        // Arc<Vec<u8>> clone (what it costs now) costs.
        const REAL_SIZE: usize = 170_699_420;
        let source: Vec<u8> = vec![0u8; REAL_SIZE];
        let vec_clone_started = std::time::Instant::now();
        let _cloned = std::hint::black_box(source.clone());
        let vec_clone_elapsed = vec_clone_started.elapsed();
        eprintln!("[measured] Vec<u8> clone at {REAL_SIZE} bytes: {vec_clone_elapsed:?}");

        let arc_source = std::sync::Arc::new(source);
        let arc_clone_started = std::time::Instant::now();
        let _arc_cloned = std::hint::black_box(arc_source.clone());
        let arc_clone_elapsed = arc_clone_started.elapsed();
        eprintln!("[measured] Arc<Vec<u8>> clone at {REAL_SIZE} bytes: {arc_clone_elapsed:?}");

        assert!(
            arc_clone_elapsed < vec_clone_elapsed,
            "an Arc clone must be a refcount bump, not a memcpy of the buffer it points to"
        );
    }
}
