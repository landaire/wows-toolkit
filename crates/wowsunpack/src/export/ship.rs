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
use crate::game_params::keys;
use crate::game_params::provider::GameMetadataProvider;
use crate::game_params::types::ArmorMap;
use crate::game_params::types::GameParamProvider;
use crate::game_params::types::MountPoint;
use crate::game_params::types::Vehicle;
use crate::models::assets_bin::PrototypeDatabase;
use crate::models::assets_bin::{
    self,
};
use crate::models::geometry;
use crate::models::model;
use crate::models::visual::VisualPrototype;
use crate::models::visual::{
    self,
};

use super::camouflage::CamouflageDb;
use super::camouflage::{
    self,
};
use super::gltf_export::InteractiveArmorMesh;
use super::gltf_export::SubModel;
use super::gltf_export::TextureSet;
use super::gltf_export::{
    self,
};
use super::texture;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

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
    /// Module overrides: component type key (e.g. "artillery") to component name.
    /// Overrides the default component for specific types.
    pub module_overrides: std::collections::HashMap<crate::game_params::keys::ComponentType, String>,
}

impl Default for ShipExportOptions {
    fn default() -> Self {
        Self { lod: 0, hull: None, textures: true, damaged: false, module_overrides: std::collections::HashMap::new() }
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
    assets_bin_bytes: Vec<u8>,
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

        Ok(Self { assets_bin_bytes, vfs: vfs.clone(), metadata, camo_db })
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

        let camo_db = CamouflageDb::load(vfs);

        Ok(Self { assets_bin_bytes, vfs: vfs.clone(), metadata, camo_db })
    }

    /// Load shared assets directly from a World of Warships installation directory.
    ///
    /// This is a convenience wrapper that builds the VFS (idx files + assets.bin overlay)
    /// from the game directory, then calls [`Self::load`]. It uses the latest build
    /// found in the `bin/` directory.
    ///
    /// For callers who already have a VFS, use [`Self::load`] instead.
    pub fn from_game_dir(game_dir: &Path) -> Result<Self, Report> {
        let vfs = crate::game_data::build_game_vfs(game_dir)?;
        Self::load(&vfs)
    }

    /// Set translations for display name resolution.
    ///
    /// # Panics
    /// Panics if the inner metadata Arc has been cloned (i.e. there are other owners).
    /// Call this before sharing the assets.
    pub fn set_translations(&mut self, catalog: gettext::Catalog) {
        Arc::get_mut(&mut self.metadata)
            .expect("cannot set translations: GameMetadataProvider is shared")
            .set_translations(catalog);
    }

    /// Access the underlying `GameMetadataProvider`.
    pub fn metadata(&self) -> &GameMetadataProvider {
        &self.metadata
    }

    /// Access the underlying VFS root.
    pub fn vfs(&self) -> &VfsPath {
        &self.vfs
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
                display_name: param
                    .and_then(|p| self.metadata.localized_name_from_param(p).map(|s: &str| s.to_string())),
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
                display_name: self.metadata.localized_name_from_param(&param).map(|s: &str| s.to_string()),
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

            let display_name = self
                .metadata
                .localized_name_from_param(param)
                .map(|s: &str| s.to_string())
                .unwrap_or_else(|| param.index().to_string());

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

    /// List available camouflage texture schemes for a ship.
    pub fn list_texture_schemes(&self, name: &str) -> Result<Vec<String>, Report> {
        let info = self.find_ship(name)?;
        let db = self.db()?;
        let self_id_index = db.build_self_id_index();

        // Collect visuals for the ship model dir.
        let visual_paths = self.find_visual_paths(&db, &self_id_index, &info.model_dir);
        let sub_models = self.load_sub_models(&db, &self_id_index, &visual_paths)?;

        // Also load turret models to include their stems.
        let vehicle = self.find_vehicle(&info.model_dir).ok();
        let mount_points = vehicle
            .and_then(|v| self.select_hull_mount_points(v, None, &std::collections::HashMap::new()))
            .unwrap_or_default();
        let turret_data = self.load_turret_models(&db, &self_id_index, &mount_points)?;

        let mut all_stems = Vec::new();
        for smd in &sub_models {
            for mfm in collect_mfm_info(&smd.visual, &db) {
                all_stems.push(mfm.stem);
            }
        }
        for tmd in &turret_data {
            for mfm in collect_mfm_info(&tmd.visual, &db) {
                all_stems.push(mfm.stem);
            }
        }

        let mut schemes = texture::discover_texture_schemes(&self.vfs, &all_stems);

        // Also include material-based camo scheme display names.
        let ship_index = self.find_ship_index(&info.model_dir);
        let ship_idx = ship_index.as_deref();
        let mat_camos = self.discover_mat_camo_schemes(&info.model_dir, ship_idx);
        for scheme in &mat_camos {
            let tag = if scheme.tiled { "tiled" } else { "mat_camo" };
            schemes.push(format!("{} ({})", scheme.display_name, tag));
        }

        // Include universal camos (PCEC entries available to all ships).
        let universal = self.discover_universal_camo_schemes(ship_idx);
        for scheme in &universal {
            let tag = if scheme.tiled { "tiled" } else { "mat_camo" };
            schemes.push(format!("{} (universal/{})", scheme.display_name, tag));
        }

        Ok(schemes)
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
            display_name: param.and_then(|p| self.metadata.localized_name_from_param(p).map(|s: &str| s.to_string())),
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
            .and_then(|v| self.select_hull_mount_points(v, options.hull.as_deref(), &options.module_overrides))
            .unwrap_or_default();

        let loaded = self.load_mounts(&db, &self_id_index, &mount_points, &hull_parts)?;
        let turret_models = loaded.turret_models;
        let mounts = loaded.mounts;

        // Resolve material-based camouflage schemes (ship-specific + universal).
        let ship_index = self.find_ship_index(&info.model_dir);
        let ship_idx = ship_index.as_deref();
        let mut mat_camo_schemes = self.discover_mat_camo_schemes(&info.model_dir, ship_idx);
        mat_camo_schemes.extend(self.discover_universal_camo_schemes(ship_idx));

        // Extract armor thickness map and hit locations from GameParams.
        let armor_map = vehicle.and_then(|v| v.armor().cloned());
        let hit_locations = vehicle.and_then(|v| v.hit_locations().cloned());

        Ok(ShipModelContext {
            vfs: self.vfs.clone(),
            assets_bin_bytes: self.assets_bin_bytes.clone(),
            hull_parts,
            turret_models,
            mounts,
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
            let Some(exterior) = param.exterior() else {
                continue;
            };
            let Some(camo_name) = exterior.camouflage() else {
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
                .localized_name_from_id(&ids_key)
                .filter(|s| s != &ids_key) // gettext returns key as-is when not found
                .or_else(|| {
                    // Fallback: try IDS_{index}
                    self.metadata
                        .localized_name_from_param(&param)
                        .filter(|s| !s.starts_with("IDS_"))
                        .map(|s| s.to_string())
                })
                .unwrap_or_else(|| camo_name.to_string());

            // Collect unique texture paths (most mat_camos reuse one texture for all parts).
            let mut unique_paths: Vec<String> = Vec::new();
            let mut seen_paths = HashSet::new();
            for path in entry.textures.values() {
                if seen_paths.insert(path.clone()) {
                    unique_paths.push(path.clone());
                }
            }

            // For tiled camos, resolve color scheme.
            let color_scheme_colors = if entry.tiled {
                entry.color_scheme.as_ref().and_then(|cs_name| camo_db.color_scheme(cs_name)).map(|cs| cs.colors)
            } else {
                None
            };

            result.push(MatCamoScheme {
                display_name,
                texture_paths: unique_paths,
                tiled: entry.tiled,
                color_scheme_colors,
                uv_transforms: entry.uv_transforms.clone(),
            });
        }

        result
    }

    /// Discover universal camouflage schemes (PCEC entries available to all ships).
    ///
    /// These are not referenced by any ship's `permoflages` list — they're
    /// universally applicable. Deduplicated by camouflage name.
    fn discover_universal_camo_schemes(&self, ship_index: Option<&str>) -> Vec<MatCamoScheme> {
        let camo_db = match &self.camo_db {
            Some(db) => db,
            None => return Vec::new(),
        };

        let mut result = Vec::new();
        let mut seen_camo_names = HashSet::new();

        for param in self.metadata.params() {
            let name = param.name();
            if !name.starts_with("PCEC") {
                continue;
            }
            let Some(exterior) = param.exterior() else {
                continue;
            };
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

            let ids_key = format!("IDS_{}", name.to_uppercase());
            let display_name = self
                .metadata
                .localized_name_from_id(&ids_key)
                .filter(|s| s != &ids_key)
                .unwrap_or_else(|| camo_name.to_string());

            let color_scheme_colors = if entry.tiled {
                entry.color_scheme.as_ref().and_then(|cs_name| camo_db.color_scheme(cs_name)).map(|cs| cs.colors)
            } else {
                None
            };

            let mut unique_paths: Vec<String> = Vec::new();
            let mut seen_paths = HashSet::new();
            for path in entry.textures.values() {
                if seen_paths.insert(path.clone()) {
                    unique_paths.push(path.clone());
                }
            }

            result.push(MatCamoScheme {
                display_name,
                texture_paths: unique_paths,
                tiled: entry.tiled,
                color_scheme_colors,
                uv_transforms: entry.uv_transforms.clone(),
            });
        }

        result
    }

    /// Re-parse the PrototypeDatabase from owned bytes.
    fn db(&self) -> Result<PrototypeDatabase<'_>, Report> {
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

    /// Find the ship param's index name (e.g. "PJSB018_Yamato_1944") from model directory.
    fn find_ship_index(&self, model_dir: &str) -> Option<String> {
        self.metadata
            .params()
            .iter()
            .find(|p| p.vehicle().and_then(|v| v.model_path()).map(|mp| mp.contains(model_dir)).unwrap_or(false))
            .map(|p| p.index().to_string())
    }

    /// Scan paths_storage for .visual files in a directory matching the name.
    fn find_visual_paths(
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
    fn load_sub_models(
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

    /// Select mount points for the chosen hull upgrade, with optional module overrides.
    fn select_hull_mount_points(
        &self,
        vehicle: &crate::game_params::types::Vehicle,
        hull_selection: Option<&str>,
        module_overrides: &std::collections::HashMap<crate::game_params::keys::ComponentType, String>,
    ) -> Option<Vec<MountPoint>> {
        let upgrades = vehicle.hull_upgrades()?;
        let mut sorted: Vec<_> = upgrades.iter().collect();
        sorted.sort_by_key(|(k, _)| (*k).clone());

        let selected = if let Some(sel) = hull_selection {
            sorted
                .iter()
                .find(|(name, _)| *name == sel || name.to_lowercase().contains(&sel.to_lowercase()))
                .or_else(|| {
                    let prefix = format!("{sel}_");
                    sorted.iter().find(|(_, config)| {
                        config
                            .component_name(keys::ComponentType::Hull)
                            .map(|n| n.starts_with(&prefix))
                            .unwrap_or(false)
                    })
                })
                .copied()
        } else {
            sorted.first().copied()
        };

        selected.map(|(_, config)| {
            if module_overrides.is_empty() {
                config.all_mount_points().cloned().collect()
            } else {
                config.mount_points_with_overrides(module_overrides).cloned().collect()
            }
        })
    }

    /// Load turret models and build mount resolution data.
    fn load_mounts(
        &self,
        db: &PrototypeDatabase<'_>,
        self_id_index: &HashMap<u64, usize>,
        mount_points: &[MountPoint],
        hull_parts: &[OwnedSubModel],
    ) -> Result<LoadedMounts, Report> {
        // Collect hardpoint transforms from hull visuals.
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
                Some(child_xform) => mat4_mul_col_major(&hull_transform, &child_xform),
            };

            // The turret model's Rotate_Y_BlendBone encodes its rest-pose
            // facing direction. To place the model correctly at the hardpoint
            // we undo this rest-pose rotation so the hardpoint's own orientation
            // takes over: visual_transform = hp_transform * inverse(bone_rotation).
            // Armor geometry is already aligned with the hardpoint, so it uses
            // the raw transform without rotation correction.
            let armor_transform = Some(hp_transform);
            let turret_visual = &turret_models[model_idx].visual;
            let yaw_correction = turret_visual
                .find_node_local_matrix("Rotate_Y_BlendBone", &db.strings)
                .map(|bone_local| mat4_rotation_inverse(&bone_local));
            let base_transform = match yaw_correction {
                Some(inv) => mat4_mul_col_major(&hp_transform, &inv),
                None => hp_transform,
            };

            let visual_transform = Some(base_transform);

            // Build per-vertex barrel pitch if pitchDeadZones applies.
            let min_pitch = mi.min_pitch_at_yaw(0.0);
            let barrel_pitch =
                if min_pitch > 0.0 { build_barrel_pitch(turret_visual, &db.strings, min_pitch) } else { None };

            mounts.push(ResolvedMount {
                hp_name: mi.hp_name().to_string(),
                turret_model_index: model_idx,
                transform: visual_transform,
                armor_transform,
                mount_armor: mi.mount_armor().cloned(),
                species: mi.species(),
                barrel_pitch,
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
    assets_bin_bytes: Vec<u8>,
    hull_parts: Vec<OwnedSubModel>,
    turret_models: Vec<OwnedSubModel>,
    mounts: Vec<ResolvedMount>,
    info: ShipInfo,
    options: ShipExportOptions,
    mat_camo_schemes: Vec<MatCamoScheme>,
    /// Armor thickness map from GameParams.  See [`ArmorMap`].
    armor_map: Option<ArmorMap>,
    /// Hit location zones from GameParams, keyed by zone name (e.g. "Citadel").
    hit_locations: Option<HashMap<String, crate::game_params::types::HitLocation>>,
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

impl ShipModelContext {
    /// Ship identity information.
    pub fn info(&self) -> &ShipInfo {
        &self.info
    }

    /// Hull part names (sub-model names).
    pub fn hull_part_names(&self) -> Vec<&str> {
        self.hull_parts.iter().map(|p| p.name.as_str()).collect()
    }

    /// Number of mounted components (turrets, AA, etc.).
    pub fn mount_count(&self) -> usize {
        self.mounts.len()
    }

    /// Number of unique turret/mount 3D models.
    pub fn unique_turret_count(&self) -> usize {
        self.turret_models.len()
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
                let mut mesh = InteractiveArmorMesh::from_armor_model(
                    armor_model,
                    self.armor_map.as_ref(),
                    mount.mount_armor.as_ref(),
                );
                mesh.transform = mount.armor_transform.map(gltf_export::negate_z_transform);
                mesh.name = format!("{} [{}]", mesh.name, mount.hp_name);
                result.push(mesh);
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
            for mesh in &mut meshes {
                mesh.transform = mount.transform.map(gltf_export::negate_z_transform);
                mesh.name = format!("{} [{}]", mesh.name, mount.hp_name);
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
                let dds_bytes = texture::load_base_albedo_bytes(&self.vfs, &mfm_path)?;
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

        // Build SubModel list.
        let mut sub_models: Vec<SubModel<'_>> = Vec::new();

        // Hull sub-models.
        for (data, geom) in self.hull_parts.iter().zip(hull_geoms.iter()) {
            sub_models.push(SubModel {
                name: data.name.clone(),
                visual: &data.visual,
                geometry: geom,
                transform: None,
                group: "Hull",
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
                group: mount_group(mount.species),
                barrel_pitch: mount.barrel_pitch.clone(),
            });
        }

        // Load textures.
        let texture_set = if self.options.textures {
            let mut all_mfm_infos = Vec::new();
            for sub in &sub_models {
                all_mfm_infos.extend(collect_mfm_info(sub.visual, &db));
            }
            let mut tex_set = build_texture_set(&all_mfm_infos, &self.vfs);
            let per_ship_count = tex_set.camo_schemes.len();

            // Merge material-based camo textures (mat_Steel, mat_Yamato_KoF, etc.).
            if !self.mat_camo_schemes.is_empty() {
                let stems: Vec<String> = {
                    let mut s = HashSet::new();
                    for info in &all_mfm_infos {
                        s.insert(info.stem.clone());
                    }
                    s.into_iter().collect()
                };

                for scheme in &self.mat_camo_schemes {
                    let mut png_bytes = None;

                    if scheme.tiled {
                        // Tiled camo: load tile DDS and bake with color scheme.
                        if let Some(colors) = &scheme.color_scheme_colors {
                            for path in &scheme.texture_paths {
                                if let Some(dds) = texture::load_dds_from_vfs(&self.vfs, path) {
                                    match texture::bake_tiled_camo_png(&dds, colors) {
                                        Ok(png) => {
                                            png_bytes = Some(png);
                                            break;
                                        }
                                        Err(e) => {
                                            eprintln!("  Warning: failed to bake tiled camo {}: {e}", path);
                                        }
                                    }
                                }
                            }
                        }
                    } else {
                        // Non-tiled mat_camo: load DDS and convert to PNG.
                        for path in &scheme.texture_paths {
                            if let Some(dds) = texture::load_dds_from_vfs(&self.vfs, path) {
                                match texture::dds_to_png(&dds) {
                                    Ok(png) => {
                                        png_bytes = Some(png);
                                        break;
                                    }
                                    Err(e) => {
                                        eprintln!("  Warning: failed to decode mat_camo texture {}: {e}", path);
                                    }
                                }
                            }
                        }
                    }

                    if let Some(png) = png_bytes {
                        let mut scheme_textures = HashMap::new();
                        for stem in &stems {
                            scheme_textures.insert(stem.clone(), png.clone());
                        }
                        let scheme_idx = tex_set.camo_schemes.len();
                        tex_set.camo_schemes.push((scheme.display_name.clone(), scheme_textures));
                        if scheme.tiled {
                            // Store per-stem UV transforms for this tiled scheme.
                            for stem in &stems {
                                let cat = camouflage::classify_part_category(stem);
                                if let Some(xform) = scheme.uv_transforms.get(cat) {
                                    tex_set.tiled_uv_transforms.insert(
                                        (scheme_idx, stem.clone()),
                                        [xform.scale[0], xform.scale[1], xform.offset[0], xform.offset[1]],
                                    );
                                }
                            }
                        }
                    }
                }

                let mat_count = tex_set.camo_schemes.len() - per_ship_count;
                eprintln!("  Texture variants: {} per-ship, {} material-based", per_ship_count, mat_count);
            }

            tex_set
        } else {
            TextureSet::empty()
        };

        // Collect armor meshes from hull AND turret geometries with thickness data.
        let armor_map = self.armor_map.as_ref();
        let mut armor_meshes: Vec<gltf_export::ArmorSubModel> = Vec::new();
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
                    s.transform = mount.armor_transform;
                    s.name = format!("{} [{}]", s.name, mount.hp_name);
                }
                armor_meshes.extend(subs);
            }
        }

        gltf_export::export_ship_glb(
            &sub_models,
            &armor_meshes,
            &db,
            self.options.lod,
            &texture_set,
            self.options.damaged,
            writer,
        )
        .context("Failed to export ship GLB")?;

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Internal types
// ---------------------------------------------------------------------------

/// Owns visual + geometry bytes for one sub-model (no lifetime parameters).
struct OwnedSubModel {
    name: String,
    visual: VisualPrototype,
    geom_bytes: Vec<u8>,
    /// Raw `.splash` file bytes (only present for base hull models).
    splash_bytes: Option<Vec<u8>>,
}

/// Result of [`ShipAssets::load_mounts`].
struct LoadedMounts {
    turret_models: Vec<OwnedSubModel>,
    #[allow(dead_code)]
    turret_model_index: HashMap<String, usize>,
    mounts: Vec<ResolvedMount>,
}

/// A mount instance with resolved transform.
struct ResolvedMount {
    hp_name: String,
    turret_model_index: usize,
    /// Visual transform with yaw correction.
    transform: Option<[f32; 16]>,
    /// Raw hardpoint transform without model rotation (for armor geometry).
    armor_transform: Option<[f32; 16]>,
    /// Per-mount armor map for turret shell surfaces (from `A_Artillery.HP_XXX.armor`).
    mount_armor: Option<crate::game_params::types::ArmorMap>,
    /// Mount species from GameParams `typeinfo.species`.
    species: Option<crate::game_params::types::MountSpecies>,
    /// Per-vertex barrel pitch configuration (if pitchDeadZones applies).
    barrel_pitch: Option<super::gltf_export::BarrelPitch>,
}

/// Pre-resolved material-based camouflage scheme (owned data, no lifetimes).
struct MatCamoScheme {
    /// Display name for the variant (translated or fallback).
    display_name: String,
    /// Albedo texture VFS paths from camouflages.xml.
    texture_paths: Vec<String>,
    /// Whether this is a tiled camo (uses UV tiling via KHR_texture_transform).
    tiled: bool,
    /// Resolved color scheme colors for tiled camos (4 RGBA colors, linear space).
    color_scheme_colors: Option<[[f32; 4]; 4]>,
    /// Per-part UV transforms for tiled camos. Key = part category (lowercase).
    uv_transforms: HashMap<String, camouflage::UvTransform>,
}

// ---------------------------------------------------------------------------
// Shared helpers (pub so main.rs export-model can use them too)
// ---------------------------------------------------------------------------

/// Resolved MFM info: stem (leaf name without `.mfm`) and full VFS path.
pub struct MfmInfo {
    pub stem: String,
    pub full_path: String,
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
            result.push(MfmInfo { stem: stem.to_string(), full_path });
        }
    }

    result
}

/// Build a `TextureSet` from MFM infos: base albedo + all camo schemes.
pub fn build_texture_set(mfm_infos: &[MfmInfo], vfs: &VfsPath) -> TextureSet {
    let mut base = HashMap::new();

    let mut seen_stems = HashSet::new();
    let mut unique_infos: Vec<&MfmInfo> = Vec::new();
    for info in mfm_infos {
        if seen_stems.insert(info.stem.clone()) {
            unique_infos.push(info);
        }
    }

    // Load base albedo textures.
    for info in &unique_infos {
        if let Some(dds_bytes) = texture::load_base_albedo_bytes(vfs, &info.full_path) {
            match texture::dds_to_png(&dds_bytes) {
                Ok(png_bytes) => {
                    base.insert(info.stem.clone(), png_bytes);
                }
                Err(e) => {
                    eprintln!("  Warning: failed to decode base texture for {}: {e}", info.stem);
                }
            }
        }
    }

    // Discover camo schemes.
    let stems: Vec<String> = unique_infos.iter().map(|i| i.stem.clone()).collect();
    let schemes = texture::discover_texture_schemes(vfs, &stems);

    let mut camo_schemes = Vec::new();
    for scheme in &schemes {
        let mut scheme_textures = HashMap::new();
        for info in &unique_infos {
            if let Some((_base_name, dds_bytes)) = texture::load_texture_bytes(vfs, &info.stem, scheme) {
                match texture::dds_to_png(&dds_bytes) {
                    Ok(png_bytes) => {
                        scheme_textures.insert(info.stem.clone(), png_bytes);
                    }
                    Err(e) => {
                        eprintln!("  Warning: failed to decode camo texture {}_{}: {e}", info.stem, scheme);
                    }
                }
            }
        }
        if !scheme_textures.is_empty() {
            camo_schemes.push((scheme.clone(), scheme_textures));
        }
    }

    TextureSet { base, camo_schemes, tiled_uv_transforms: HashMap::new() }
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
    // Find the longest hull HP name that is a proper prefix of hp_name with
    // a '_' separator. This avoids partial matches like HP_AG matching HP_AGM_3.
    let mut best_parent: Option<(&str, &[f32; 16])> = None;
    for (hull_hp, xform) in hp_transforms {
        if hp_name.len() > hull_hp.len()
            && hp_name.starts_with(hull_hp.as_str())
            && hp_name.as_bytes()[hull_hp.len()] == b'_'
            && best_parent.is_none_or(|(bp, _)| hull_hp.len() > bp.len())
        {
            best_parent = Some((hull_hp.as_str(), xform));
        }
    }
    let (parent_hp, parent_xform) = best_parent?;

    // Extract child HP name: everything after "parent_"
    let child_hp = &hp_name[parent_hp.len() + 1..];

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

fn mat4_mul_col_major(a: &[f32; 16], b: &[f32; 16]) -> [f32; 16] {
    let mut out = [0.0f32; 16];
    for col in 0..4 {
        for row in 0..4 {
            out[col * 4 + row] = (0..4).map(|k| a[k * 4 + row] * b[col * 4 + k]).sum();
        }
    }
    out
}

/// Extract the rotation part of a column-major 4x4 matrix and return its
/// inverse (transpose) as a full 4x4 matrix with zero translation.
///
/// Valid for rigid-body transforms (orthonormal rotation + translation).
/// The inverse of the rotation part is simply its transpose.
fn mat4_rotation_inverse(m: &[f32; 16]) -> [f32; 16] {
    // Column-major layout:
    //   col0 = m[0..4], col1 = m[4..8], col2 = m[8..12], col3 = m[12..16]
    // 3x3 rotation at (row, col) -> m[col*4 + row]
    // Transpose: swap (row, col) -> (col, row)
    [
        m[0], m[4], m[8], 0.0, // col 0 = original row 0
        m[1], m[5], m[9], 0.0, // col 1 = original row 1
        m[2], m[6], m[10], 0.0, // col 2 = original row 2
        0.0, 0.0, 0.0, 1.0, // no translation
    ]
}

/// Map a mount's species to a display group name.
fn mount_group(species: Option<crate::game_params::types::MountSpecies>) -> &'static str {
    match species {
        Some(s) => s.display_group(),
        None => "Other",
    }
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
