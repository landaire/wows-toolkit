//! Background ship-armor load: ports the egui app's `load_ship_armor`
//! (`armor_viewer/common.rs:137-315`) plus the default-hull option building
//! from `load_ship_for_pane_with_lod` (`armor_viewer/ui/tab.rs:2374`).
//!
//! **Default load.** [`spawn_load_ship_armor`]/[`load_ship_armor_by_param`]
//! load the DEFAULT hull at the DEFAULT (highest-detail) LOD -- `hull_lod: 0`
//! and `selected_hull: None` both match the egui app's own per-pane defaults
//! (`ArmorPane::with_defaults`, `armor_viewer/state.rs`), and
//! `ShipExportOptions::hull: None` resolves to the first/stock hull the same
//! way the egui app's own default load does.
//!
//! **Reload (Milestone 4 Task 8c).** [`spawn_reload_ship_armor`]/
//! [`build_reload_options`] instead take an explicit hull/LOD/module
//! selection from `viewport_view::ViewportView::reload_ship`, which the
//! egui app's own incremental `start_hull_lod_reload`/`start_upgrade_reload`
//! (`tab.rs:2472`/`2568`) mutate an existing `LoadedShipArmor` in place for;
//! this port's `LoadedShipArmor` is an immutable `Arc`, so a reload always
//! produces a brand new one via the same `load_ship_armor` this module's
//! default load uses, rather than a second, narrower load path.
//!
//! **Deferred fields.** The egui `LoadedShipArmor` also carries splash-box
//! data, hit-location data, and camera orbit trajectories -- all
//! analysis-only, consumed by UI this milestone does not build yet (splash
//! analysis, camera ellipse overlay). This port's [`LoadedShipArmor`] omits
//! them outright rather than carrying `None`/empty placeholders, keeping the
//! struct to exactly what the sidebar/load/reload/upload path this milestone
//! builds produces or consumes.
//!
//! **Kept fields.** `hull_meshes`/`hull_part_groups`/`hull_textures`/
//! `hull_lod_count` are loaded (mirroring the egui core). `hull_upgrade_names`
//! and `module_alternatives` are cheap to derive alongside a load/reload (same
//! source data, no extra ship export), so they are populated for the hull
//! popover's selectors (`popover.rs`) to consume directly.

use std::collections::HashMap;
use std::sync::Arc;

use gpui::App;
use gpui::AppContext;
use gpui::Task;

use wowsunpack::export::camo_textures::CamoSchemeInfo;
use wowsunpack::export::camo_textures::CamoTextureSource;
use wowsunpack::export::gltf_export::InteractiveArmorMesh;
use wowsunpack::export::gltf_export::InteractiveHullMesh;
use wowsunpack::export::ship::ShipAssets;
use wowsunpack::export::ship::ShipExportOptions;
use wowsunpack::game_params::keys::ComponentType;
use wowsunpack::game_params::types::GameParamProvider;
use wowsunpack::game_params::types::Vehicle;

use crate::viewport::types::Vec3;

/// Key identifying a specific plate: (zone, material_name, thickness in
/// tenths of mm). Matches the egui app's `PlateKey` (`armor_viewer/state.rs`);
/// the thickness discriminator ensures plate-boundary edges stop at real
/// thickness changes.
pub type PlateKey = (String, String, i32);

/// A material/part within an armor zone, with its sorted unique plate
/// thicknesses. Ports `armor_viewer::state::ZonePart` verbatim.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ZonePart {
    pub name: String,
    /// Sorted unique plate thicknesses in tenths of mm.
    pub plates: Vec<i32>,
}

/// An armor zone containing multiple material parts, each with plate
/// thicknesses. Ports `armor_viewer::state::ArmorZone` verbatim.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArmorZone {
    pub name: String,
    pub parts: Vec<ZonePart>,
}

/// Per-triangle metadata carried alongside an uploaded mesh, for later
/// picking/tooltip use. Ports `armor_viewer::state::ArmorTriangleTooltip`.
#[derive(Clone, Debug)]
pub struct ArmorTriangleTooltip {
    pub material_name: String,
    pub zone: String,
    pub thickness_mm: f32,
    /// Per-layer thicknesses (ordered by model_index). Single-layer plates
    /// have one entry.
    pub layers: Vec<f32>,
    pub color: [f32; 4],
}

/// Reasons a ship's armor model failed to load.
#[derive(Debug, Clone, thiserror::Error)]
pub enum ShipLoadError {
    #[error("no vehicle found for param index {0}")]
    NoVehicle(String),
    #[error("failed to export ship model: {0}")]
    ShipExport(String),
    #[error("failed to build armor meshes: {0}")]
    ArmorMeshes(String),
    #[error("failed to build hull meshes: {0}")]
    HullMeshes(String),
    #[error("failed to resolve camouflage textures: {0}")]
    CamoSource(String),
}

/// Reasons a GLB model export ([`export_ship_glb`]) failed.
#[derive(Debug, thiserror::Error)]
pub enum ExportGlbError {
    #[error("no vehicle found for param index {0}")]
    NoVehicle(String),
    #[error("failed to export ship model: {0}")]
    ShipExport(String),
    #[error("failed to create output file {path}: {source}")]
    CreateFile { path: std::path::PathBuf, source: std::io::Error },
    #[error("failed to write glb: {0}")]
    WriteGlb(String),
}

/// Options for [`load_ship_armor`]. Built by [`default_load_options`] for a
/// default (stock hull, highest-detail LOD) load, or by
/// [`build_reload_options`]/[`reload_load_options`] for a Task 8c reload
/// against an explicit hull/LOD/module selection.
pub struct ShipLoadOptions {
    pub display_name: String,
    pub lod: usize,
    pub selected_hull: Option<String>,
    pub module_overrides: HashMap<ComponentType, String>,
    pub module_alternatives: Vec<(ComponentType, Vec<String>)>,
    pub hull_upgrade_names: Vec<(String, String)>,
}

/// A loaded ship's armor data: everything the sidebar/load/upload path in
/// this milestone produces or consumes. See the module doc for which fields
/// of the egui app's `LoadedShipArmor` this intentionally omits.
pub struct LoadedShipArmor {
    pub display_name: String,
    pub meshes: Vec<InteractiveArmorMesh>,
    pub bounds: (Vec3, Vec3),
    pub zones: Vec<String>,
    /// Ordered mapping: zone name -> sorted list of unique material names in that zone.
    pub zone_parts: Vec<(String, Vec<String>)>,
    /// Three-level hierarchy: zone -> materials -> sorted unique plate thicknesses.
    pub zone_part_plates: Vec<ArmorZone>,
    /// Hull visual meshes. Loaded for Milestone 4; not displayed by `upload.rs` yet.
    pub hull_meshes: Vec<InteractiveHullMesh>,
    pub hull_part_groups: Vec<(String, Vec<String>)>,
    /// Decoded hull textures: mfm_path -> (width, height, RGBA8 pixels).
    pub hull_textures: HashMap<String, (u32, u32, Vec<u8>)>,
    pub hull_lod_count: usize,
    pub hull_lod: usize,
    /// Available hull upgrade names: Vec<(param_key, display_label)>, sorted alphabetically.
    pub hull_upgrade_names: Vec<(String, String)>,
    /// The hull upgrade key that was used to load this armor data. `None` = stock.
    pub loaded_hull: Option<String>,
    /// Module alternatives for the loaded hull upgrade: component type -> component names.
    pub module_alternatives: Vec<(ComponentType, Vec<String>)>,
    /// Camo scheme metadata for the picker (`popover.rs`); no textures decoded.
    pub camo_scheme_infos: Vec<CamoSchemeInfo>,
    /// Decodes a scheme's textures on demand when one is selected.
    pub camo_source: CamoTextureSource,
}

impl LoadedShipArmor {
    /// Bounding-box center in model space.
    pub fn center(&self) -> Vec3 {
        (self.bounds.0 + self.bounds.1) * 0.5
    }
}

/// Apply a column-major 4x4 transform to a point (position). Ports
/// `armor_viewer::ui::tab::transform_point` verbatim.
pub(crate) fn transform_point(t: &[f32; 16], p: [f32; 3]) -> [f32; 3] {
    [
        t[0] * p[0] + t[4] * p[1] + t[8] * p[2] + t[12],
        t[1] * p[0] + t[5] * p[1] + t[9] * p[2] + t[13],
        t[2] * p[0] + t[6] * p[1] + t[10] * p[2] + t[14],
    ]
}

/// Apply the upper-left 3x3 of a column-major 4x4 transform to a normal and
/// renormalize. Ports `armor_viewer::ui::tab::transform_normal` verbatim.
pub(crate) fn transform_normal(t: &[f32; 16], n: [f32; 3]) -> [f32; 3] {
    let x = t[0] * n[0] + t[4] * n[1] + t[8] * n[2];
    let y = t[1] * n[0] + t[5] * n[1] + t[9] * n[2];
    let z = t[2] * n[0] + t[6] * n[1] + t[10] * n[2];
    let len = (x * x + y * y + z * z).sqrt();
    if len < 1e-10 { [0.0, 0.0, 0.0] } else { [x / len, y / len, z / len] }
}

/// Classifies a hull render-set name into a display group, by its `[HP_...]`
/// mount tag. Ports `armor_viewer::ui::tab::hull_part_group` verbatim.
fn hull_part_group(name: &str) -> &'static str {
    if let Some(start) = name.find("[HP_") {
        let hp = &name[start + 1..name.len() - 1];
        if hp.starts_with("HP_AGM") {
            "Main Battery"
        } else if hp.starts_with("HP_AGS") {
            "Secondary Battery"
        } else if hp.starts_with("HP_AGA") {
            "AA Guns"
        } else if hp.starts_with("HP_ATB") || hp.starts_with("HP_AT_") {
            "Torpedoes"
        } else {
            "Other"
        }
    } else {
        "Hull"
    }
}

/// Fixed display order for hull part groups. Ports
/// `armor_viewer::ui::tab::hull_group_order` verbatim.
fn hull_group_order(group: &str) -> u32 {
    match group {
        "Hull" => 0,
        "Main Battery" => 1,
        "Secondary Battery" => 2,
        "AA Guns" => 3,
        "Torpedoes" => 4,
        "Other" => 5,
        _ => 6,
    }
}

/// Build hull part groups from a list of hull meshes. Ports
/// `armor_viewer::ui::tab::build_hull_part_groups` verbatim.
pub(crate) fn build_hull_part_groups(hull_meshes: &[InteractiveHullMesh]) -> Vec<(String, Vec<String>)> {
    use std::collections::BTreeSet;

    let mut group_map: HashMap<&str, BTreeSet<String>> = HashMap::new();
    for mesh in hull_meshes {
        let group = hull_part_group(&mesh.name);
        group_map.entry(group).or_default().insert(mesh.name.clone());
    }

    let mut groups: Vec<(String, Vec<String>)> =
        group_map.into_iter().map(|(group, names)| (group.to_string(), names.into_iter().collect())).collect();
    groups.sort_by_key(|(g, _)| hull_group_order(g));
    groups
}

/// Build sorted hull upgrade labels with diff-based suffixes. Ports
/// `armor_viewer::common::build_hull_upgrade_names` verbatim.
fn build_hull_upgrade_names(vehicle: &Vehicle) -> Vec<(String, String)> {
    vehicle
        .hull_upgrades()
        .map(|upgrades| {
            let mut sorted: Vec<_> = upgrades.iter().collect();
            sorted.sort_by_key(|(k, _)| (*k).clone());
            let base = &sorted[0].1;
            sorted
                .iter()
                .enumerate()
                .map(|(i, (k, config))| {
                    let letter = (b'A' + i as u8) as char;
                    let diffs: Vec<String> = ComponentType::ALL
                        .iter()
                        .filter(|&&ct| ct != ComponentType::Hull)
                        .filter(|&&ct| config.component_name(ct) != base.component_name(ct))
                        .map(|ct| ct.to_string())
                        .collect();
                    let label = if diffs.is_empty() || i == 0 {
                        format!("{letter}")
                    } else {
                        format!("{letter} ({})", diffs.join(", "))
                    };
                    ((*k).clone(), label)
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Module alternatives for `selected_hull` (or the stock/first-alphabetical
/// hull when `None`): component types with more than one option. Ports the
/// module-alternatives resolution `load_ship_for_pane_with_lod` runs before
/// every ship load/reload (`armor_viewer/ui/tab.rs:2427-2444`) verbatim.
/// Used both for a fresh ship's default load (`selected_hull: None`) and a
/// Milestone 4 Task 8c reload against a specific hull -- `module_alternatives`
/// depends on which hull is selected (each upgrade can offer different
/// component options), so a reload must re-derive this rather than carry a
/// stale value from the previously loaded armor.
fn module_alternatives_for_hull(vehicle: &Vehicle, selected_hull: Option<&str>) -> Vec<(ComponentType, Vec<String>)> {
    vehicle
        .hull_upgrades()
        .and_then(|upgrades| {
            let config = if let Some(sel) = selected_hull {
                upgrades.get(sel)
            } else {
                let mut keys: Vec<&String> = upgrades.keys().collect();
                keys.sort();
                keys.first().and_then(|k| upgrades.get(*k))
            }?;
            Some(config.component_alternatives.iter().map(|(k, v)| (*k, v.clone())).collect())
        })
        .unwrap_or_default()
}

/// Builds a default (stock hull, `lod`) [`ShipLoadOptions`] for `vehicle`.
fn default_load_options(vehicle: &Vehicle, display_name: &str, lod: usize) -> ShipLoadOptions {
    ShipLoadOptions {
        display_name: display_name.to_string(),
        lod,
        selected_hull: None,
        module_overrides: HashMap::new(),
        module_alternatives: module_alternatives_for_hull(vehicle, None),
        hull_upgrade_names: build_hull_upgrade_names(vehicle),
    }
}

/// Builds [`ShipLoadOptions`] for a Milestone 4 Task 8c reload: re-derives
/// `hull_upgrade_names` (unaffected by hull selection) and
/// `module_alternatives` (re-derived for `selected_hull` specifically, see
/// [`module_alternatives_for_hull`]'s doc) from `vehicle`, so the popover's
/// selectors reflect the new selection instead of stale data carried over
/// from the previous load.
fn reload_load_options(
    vehicle: &Vehicle,
    display_name: String,
    lod: usize,
    selected_hull: Option<String>,
    module_overrides: HashMap<ComponentType, String>,
) -> ShipLoadOptions {
    ShipLoadOptions {
        display_name,
        lod,
        module_alternatives: module_alternatives_for_hull(vehicle, selected_hull.as_deref()),
        hull_upgrade_names: build_hull_upgrade_names(vehicle),
        selected_hull,
        module_overrides,
    }
}

/// Loads a ship's armor model. Ports `armor_viewer::common::load_ship_armor`
/// verbatim, minus the splash-data/hit-location/camera-trajectory branches
/// (see the module doc for why this v1 struct omits them).
fn load_ship_armor(
    vehicle: &Vehicle,
    ship_assets: &ShipAssets,
    options: ShipLoadOptions,
) -> Result<LoadedShipArmor, ShipLoadError> {
    let export_options = ShipExportOptions {
        lod: options.lod,
        hull: options.selected_hull.clone(),
        textures: false,
        damaged: false,
        module_overrides: options.module_overrides,
    };
    let ctx = ship_assets
        .load_ship_from_vehicle(vehicle, &export_options)
        .map_err(|e| ShipLoadError::ShipExport(format!("{e:?}")))?;

    let meshes = ctx.interactive_armor_meshes().map_err(|e| ShipLoadError::ArmorMeshes(format!("{e:?}")))?;

    let mut min = [f32::MAX; 3];
    let mut max = [f32::MIN; 3];
    for mesh in &meshes {
        for pos in &mesh.positions {
            let p = match &mesh.transform {
                Some(t) => transform_point(t, *pos),
                None => *pos,
            };
            for i in 0..3 {
                min[i] = min[i].min(p[i]);
                max[i] = max[i].max(p[i]);
            }
        }
    }

    let mut zone_parts_map: HashMap<String, std::collections::HashSet<String>> = HashMap::new();
    let mut zone_part_plates_map: HashMap<String, HashMap<String, std::collections::BTreeSet<i32>>> = HashMap::new();
    for mesh in &meshes {
        for info in &mesh.triangle_info {
            zone_parts_map.entry(info.zone.clone()).or_default().insert(info.material_name.clone());
            let thickness_key = (info.thickness_mm * 10.0).round() as i32;
            zone_part_plates_map
                .entry(info.zone.clone())
                .or_default()
                .entry(info.material_name.clone())
                .or_default()
                .insert(thickness_key);
        }
    }
    let mut zone_parts: Vec<(String, Vec<String>)> = zone_parts_map
        .into_iter()
        .map(|(zone, parts)| {
            let mut parts: Vec<String> = parts.into_iter().collect();
            parts.sort();
            (zone, parts)
        })
        .collect();
    zone_parts.sort_by(|a, b| a.0.cmp(&b.0));

    let zone_part_plates: Vec<ArmorZone> = zone_parts
        .iter()
        .map(|(zone, parts)| {
            let parts_with_plates = parts
                .iter()
                .map(|part| {
                    let plates = zone_part_plates_map
                        .get(zone)
                        .and_then(|m| m.get(part))
                        .map(|s| s.iter().copied().collect())
                        .unwrap_or_default();
                    ZonePart { name: part.clone(), plates }
                })
                .collect();
            ArmorZone { name: zone.clone(), parts: parts_with_plates }
        })
        .collect();

    let zones: Vec<String> = zone_parts.iter().map(|(z, _)| z.clone()).collect();

    let hull_meshes = ctx.interactive_hull_meshes().map_err(|e| ShipLoadError::HullMeshes(format!("{e:?}")))?;

    for mesh in &hull_meshes {
        for pos in &mesh.positions {
            let p = match &mesh.transform {
                Some(t) => transform_point(t, *pos),
                None => *pos,
            };
            for i in 0..3 {
                min[i] = min[i].min(p[i]);
                max[i] = max[i].max(p[i]);
            }
        }
    }

    let hull_part_groups = build_hull_part_groups(&hull_meshes);

    let mut hull_textures = HashMap::new();
    for (mfm, png) in ctx.hull_base_albedos(&hull_meshes) {
        if let Ok(img) = image::load_from_memory(&png) {
            let rgba = img.to_rgba8();
            let (w, h) = (rgba.width(), rgba.height());
            hull_textures.insert(mfm, (w, h, rgba.into_raw()));
        }
    }

    let hull_lod_count = ctx.hull_lod_count();

    let camo_source = ctx.camo_texture_source().map_err(|e| ShipLoadError::CamoSource(format!("{e:?}")))?;
    let mut camo_scheme_infos = camo_source.scheme_infos();
    {
        let mut counts: HashMap<String, u32> = HashMap::new();
        for info in &mut camo_scheme_infos {
            let n = counts.entry(info.display_name.clone()).or_insert(0);
            *n += 1;
            if *n > 1 {
                info.display_name = format!("{} ({})", info.display_name, *n);
            }
        }
    }

    Ok(LoadedShipArmor {
        display_name: options.display_name,
        meshes,
        bounds: (Vec3::new(min[0], min[1], min[2]), Vec3::new(max[0], max[1], max[2])),
        zones,
        zone_parts,
        zone_part_plates,
        hull_meshes,
        hull_part_groups,
        hull_textures,
        hull_lod_count,
        hull_lod: options.lod,
        hull_upgrade_names: options.hull_upgrade_names,
        loaded_hull: options.selected_hull,
        module_alternatives: options.module_alternatives,
        camo_scheme_infos,
        camo_source,
    })
}

/// Default LOD level for a fresh ship load: 0 = highest detail, matching the
/// egui app's own per-pane default (`ArmorPane::with_defaults`'s `hull_lod: 0`,
/// `armor_viewer/state.rs`).
pub const DEFAULT_LOD: usize = 0;

/// Resolves `param_index`'s `Vehicle` from `ship_assets`. Shared by
/// [`load_ship_armor_by_param`] (default load) and
/// [`load_ship_armor_with_options`] (Milestone 4 Task 8c reload).
fn resolve_vehicle(ship_assets: &ShipAssets, param_index: &str) -> Result<Vehicle, ShipLoadError> {
    let param = ship_assets.metadata().game_param_by_index(param_index);
    param.as_ref().and_then(|p| p.vehicle().cloned()).ok_or_else(|| ShipLoadError::NoVehicle(param_index.to_string()))
}

/// Resolves `param_index`'s `Vehicle` from `ship_assets`, then loads its
/// default-hull armor model at [`DEFAULT_LOD`]. Synchronous and CPU/IO-bound;
/// callers run it off the UI thread (see [`spawn_load_ship_armor`]).
pub(crate) fn load_ship_armor_by_param(
    ship_assets: &ShipAssets,
    param_index: &str,
    display_name: &str,
) -> Result<LoadedShipArmor, ShipLoadError> {
    let vehicle = resolve_vehicle(ship_assets, param_index)?;
    let options = default_load_options(&vehicle, display_name, DEFAULT_LOD);
    load_ship_armor(&vehicle, ship_assets, options)
}

/// Resolves `param_index`'s `Vehicle` from `ship_assets`, then loads its
/// armor model under caller-supplied `options` -- a Milestone 4 Task 8c
/// reload with a specific hull/LOD/module selection, as opposed to
/// [`load_ship_armor_by_param`]'s always-default load. Synchronous and
/// CPU/IO-bound; callers run it off the UI thread (see
/// [`spawn_reload_ship_armor`]).
fn load_ship_armor_with_options(
    ship_assets: &ShipAssets,
    param_index: &str,
    options: ShipLoadOptions,
) -> Result<LoadedShipArmor, ShipLoadError> {
    let vehicle = resolve_vehicle(ship_assets, param_index)?;
    load_ship_armor(&vehicle, ship_assets, options)
}

/// Resolves `param_index`'s `Vehicle` from `ship_assets` and builds the
/// [`ShipLoadOptions`] a Task 8c reload needs (see [`reload_load_options`]'s
/// doc). Runs synchronously on the UI thread -- a `GameParams` lookup plus a
/// small hashmap scan, mirroring the sync prep the egui app's own
/// `load_ship_for_pane_with_lod` runs before spawning its background reload
/// (`armor_viewer/ui/tab.rs:2427-2444`) -- so the caller can build this right
/// before handing it to [`spawn_reload_ship_armor`]'s background task.
pub(crate) fn build_reload_options(
    ship_assets: &ShipAssets,
    param_index: &str,
    display_name: String,
    lod: usize,
    selected_hull: Option<String>,
    module_overrides: HashMap<ComponentType, String>,
) -> Result<ShipLoadOptions, ShipLoadError> {
    let vehicle = resolve_vehicle(ship_assets, param_index)?;
    Ok(reload_load_options(&vehicle, display_name, lod, selected_hull, module_overrides))
}

/// Default save-file name for a GLB export (Milestone 5 Task 10), matching
/// the egui app's own `format!("{}.glb", display_name)` default
/// (`armor_viewer/ui/tab.rs:1057`).
pub(crate) fn default_export_filename(display_name: &str) -> String {
    format!("{display_name}.glb")
}

/// Builds [`ShipExportOptions`] for a GLB export of the ship as currently
/// DISPLAYED in a viewport pane: the pane's current hull/LOD/module
/// selection, matching [`load_ship_armor`]'s own construction (`textures:
/// false, damaged: false`) so the exported model matches what is on screen.
/// A pure function, split out from [`export_ship_glb`] so it is unit-testable
/// without a `ShipAssets`/`Vehicle`.
pub(crate) fn export_options_from_selection(
    lod: usize,
    selected_hull: Option<String>,
    module_overrides: HashMap<ComponentType, String>,
) -> ShipExportOptions {
    ShipExportOptions { lod, hull: selected_hull, textures: false, damaged: false, module_overrides }
}

/// Exports `param_index`'s ship model to `path` as a glTF Binary (GLB) file,
/// under `options` (built by [`export_options_from_selection`] for a
/// viewport-pane export, so the exported model matches what that pane
/// currently displays). Synchronous and CPU/IO-bound; callers run it off the
/// UI thread (`viewport_view::ViewportView::confirm_export`), matching every
/// other `ShipAssets`-driven load in this module.
pub(crate) fn export_ship_glb(
    ship_assets: &ShipAssets,
    param_index: &str,
    options: &ShipExportOptions,
    path: &std::path::Path,
) -> Result<(), ExportGlbError> {
    let vehicle =
        resolve_vehicle(ship_assets, param_index).map_err(|_| ExportGlbError::NoVehicle(param_index.to_string()))?;
    let ctx = ship_assets
        .load_ship_from_vehicle(&vehicle, options)
        .map_err(|e| ExportGlbError::ShipExport(format!("{e:?}")))?;
    let mut file = std::fs::File::create(path)
        .map_err(|source| ExportGlbError::CreateFile { path: path.to_path_buf(), source })?;
    ctx.export_glb(&mut file).map_err(|e| ExportGlbError::WriteGlb(format!("{e:?}")))?;
    Ok(())
}

/// Kicks off a ship's armor load on the background executor. `bundle` is the
/// Armor Viewer's already-loaded [`ArmorAssetsBundle`](super::assets::ArmorAssetsBundle)
/// (shared `Arc`, cheap to clone into the background task); this never
/// triggers a second game-data or `ShipAssets` load.
pub fn spawn_load_ship_armor(
    bundle: Arc<super::assets::ArmorAssetsBundle>,
    param_index: String,
    display_name: String,
    cx: &App,
) -> Task<Result<LoadedShipArmor, ShipLoadError>> {
    cx.background_spawn(async move { load_ship_armor_by_param(&bundle.assets, &param_index, &display_name) })
}

/// Kicks off a background reload of `param_index`'s armor under `options` --
/// a hull-upgrade/LOD/module selection change from the hull popover
/// (`popover.rs`), driven by `viewport_view::ViewportView::reload_ship`. Like
/// [`spawn_load_ship_armor`], but the caller controls `options` (build it via
/// [`build_reload_options`]) instead of getting the always-default load;
/// `bundle` is the same shared `ArmorAssetsBundle`, so this never triggers a
/// second game-data or `ShipAssets` load either.
pub fn spawn_reload_ship_armor(
    bundle: Arc<super::assets::ArmorAssetsBundle>,
    param_index: String,
    options: ShipLoadOptions,
    cx: &App,
) -> Task<Result<LoadedShipArmor, ShipLoadError>> {
    cx.background_spawn(async move { load_ship_armor_with_options(&bundle.assets, &param_index, options) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_export_filename_appends_glb_extension() {
        assert_eq!(default_export_filename("Yamato"), "Yamato.glb");
        assert_eq!(default_export_filename("USS Iowa"), "USS Iowa.glb");
    }

    #[test]
    fn export_options_from_selection_carries_the_pane_s_live_selection_with_no_baked_textures_or_damage() {
        let mut modules = HashMap::new();
        modules.insert(ComponentType::Artillery, "AB1_Artillery".to_string());

        let options = export_options_from_selection(2, Some("B_Hull".to_string()), modules.clone());

        assert_eq!(options.lod, 2);
        assert_eq!(options.hull, Some("B_Hull".to_string()));
        assert_eq!(options.module_overrides, modules);
        assert!(!options.textures, "an interactive-viewport export never needs baked textures");
        assert!(!options.damaged, "export always targets the intact hull, matching load_ship_armor");
    }

    #[test]
    fn hull_part_group_classifies_known_hp_prefixes_and_falls_back_to_hull() {
        assert_eq!(hull_part_group("Turret[HP_AGM_1]"), "Main Battery");
        assert_eq!(hull_part_group("Turret[HP_AGS_1]"), "Secondary Battery");
        assert_eq!(hull_part_group("Mount[HP_AGA_1]"), "AA Guns");
        assert_eq!(hull_part_group("Mount[HP_ATB_1]"), "Torpedoes");
        assert_eq!(hull_part_group("Mount[HP_AT_1]"), "Torpedoes");
        assert_eq!(hull_part_group("Mount[HP_XYZ_1]"), "Other");
        assert_eq!(hull_part_group("Hull_Main"), "Hull");
    }

    #[test]
    fn hull_group_order_ranks_hull_first_and_unknown_groups_last() {
        assert_eq!(hull_group_order("Hull"), 0);
        assert_eq!(hull_group_order("Main Battery"), 1);
        assert_eq!(hull_group_order("Secondary Battery"), 2);
        assert_eq!(hull_group_order("AA Guns"), 3);
        assert_eq!(hull_group_order("Torpedoes"), 4);
        assert_eq!(hull_group_order("Other"), 5);
        assert_eq!(hull_group_order("Nonsense"), 6);
    }

    #[test]
    fn build_hull_part_groups_sorts_by_fixed_order_and_dedupes_within_a_group() {
        let meshes = vec![
            InteractiveHullMesh {
                name: "Mount[HP_AGM_1]".to_string(),
                positions: Vec::new(),
                normals: Vec::new(),
                uvs: Vec::new(),
                indices: Vec::new(),
                mfm_path: None,
                mfm_path_id: 0,
                colors: Vec::new(),
                transform: None,
            },
            InteractiveHullMesh {
                name: "Hull_Main".to_string(),
                positions: Vec::new(),
                normals: Vec::new(),
                uvs: Vec::new(),
                indices: Vec::new(),
                mfm_path: None,
                mfm_path_id: 0,
                colors: Vec::new(),
                transform: None,
            },
            InteractiveHullMesh {
                name: "Hull_Main".to_string(),
                positions: Vec::new(),
                normals: Vec::new(),
                uvs: Vec::new(),
                indices: Vec::new(),
                mfm_path: None,
                mfm_path_id: 0,
                colors: Vec::new(),
                transform: None,
            },
        ];
        let groups = build_hull_part_groups(&meshes);
        let group_names: Vec<&str> = groups.iter().map(|(g, _)| g.as_str()).collect();
        assert_eq!(group_names, vec!["Hull", "Main Battery"]);
        let hull_parts = &groups.iter().find(|(g, _)| g == "Hull").unwrap().1;
        assert_eq!(hull_parts, &vec!["Hull_Main".to_string()], "duplicate mesh names within a group must dedupe");
    }

    #[test]
    fn transform_point_applies_translation() {
        #[rustfmt::skip]
        let identity_translated: [f32; 16] = [
            1.0, 0.0, 0.0, 0.0,
            0.0, 1.0, 0.0, 0.0,
            0.0, 0.0, 1.0, 0.0,
            5.0, 6.0, 7.0, 1.0,
        ];
        let p = transform_point(&identity_translated, [1.0, 2.0, 3.0]);
        assert_eq!(p, [6.0, 8.0, 10.0]);
    }

    #[test]
    fn transform_normal_renormalizes_and_zeroes_out_degenerate_input() {
        #[rustfmt::skip]
        let scale_2x: [f32; 16] = [
            2.0, 0.0, 0.0, 0.0,
            0.0, 2.0, 0.0, 0.0,
            0.0, 0.0, 2.0, 0.0,
            0.0, 0.0, 0.0, 1.0,
        ];
        let n = transform_normal(&scale_2x, [1.0, 0.0, 0.0]);
        assert!((n[0] - 1.0).abs() < 1e-6, "expected a renormalized unit normal, got {n:?}");

        let zero: [f32; 16] = [0.0; 16];
        assert_eq!(transform_normal(&zero, [1.0, 0.0, 0.0]), [0.0, 0.0, 0.0]);
    }

    /// Needs a local game install: loads one real ship's armor model end to
    /// end (`ShipAssets` -> vehicle resolution -> `load_ship_armor`) against
    /// the latest installed build, exercising the exact synchronous path
    /// `spawn_load_ship_armor` wraps. Picks the first ship the real catalog
    /// produces rather than hardcoding a param index, so this keeps working
    /// as GameParams data changes build to build. Mirrors `assets.rs`'s own
    /// real-install test. Run with:
    ///
    /// ```text
    /// WOWS_ARMOR_VIEWER_LOAD_TEST_DIR="E:\WoWs\World_of_Warships" \
    /// cargo test -p wows-toolkit-gpui -- --ignored load_ship_armor_against_a_real_install_produces_a_renderable_model
    /// ```
    #[test]
    #[ignore = "needs a local game install; see the doc comment for the run command"]
    fn load_ship_armor_against_a_real_install_produces_a_renderable_model() {
        let wows_dir = std::env::var("WOWS_ARMOR_VIEWER_LOAD_TEST_DIR")
            .expect("set WOWS_ARMOR_VIEWER_LOAD_TEST_DIR to a WoWs install directory");
        let wows_dir = std::path::PathBuf::from(wows_dir);

        let available =
            wowsunpack::game_data::list_available_builds(&wows_dir).expect("failed to list installed builds");
        let build = *available.last().expect("expected at least one installed build");

        let vfs = wowsunpack::game_data::build_game_vfs_for_build(&wows_dir, build)
            .expect("failed to build the game VFS for the latest installed build");
        let metadata = Arc::new(
            wowsunpack::game_params::provider::GameMetadataProvider::from_vfs(&vfs)
                .expect("failed to build GameMetadataProvider from the VFS"),
        );
        let ship_assets = ShipAssets::from_vfs_with_metadata(&vfs, Arc::clone(&metadata))
            .expect("failed to load ShipAssets from the VFS");

        let catalog = crate::armor_viewer::catalog::ShipCatalog::build(&metadata);
        let ship = catalog
            .nations
            .iter()
            .flat_map(|n| &n.classes)
            .flat_map(|c| &c.ships)
            .next()
            .expect("expected at least one ship in the real catalog");

        let armor = load_ship_armor_by_param(&ship_assets, &ship.param_index, &ship.display_name)
            .unwrap_or_else(|e| panic!("failed to load armor for {} ({}): {e}", ship.display_name, ship.param_index));

        assert!(!armor.meshes.is_empty(), "expected at least one armor mesh");
        assert!(!armor.zones.is_empty(), "expected at least one armor zone");
        assert!(
            armor.bounds.0.x < armor.bounds.1.x && armor.bounds.0.y < armor.bounds.1.y,
            "expected a non-degenerate bounding box, got {:?}",
            armor.bounds
        );

        println!(
            "loaded {} ({}, build {build}): {} armor meshes, {} zones, {} hull meshes, bounds {:?}",
            armor.display_name,
            ship.param_index,
            armor.meshes.len(),
            armor.zones.len(),
            armor.hull_meshes.len(),
            armor.bounds
        );
    }

    /// Needs a local game install: exercises the exact synchronous path
    /// `viewport_view::ViewportView::confirm_export`'s background task runs
    /// (`export_options_from_selection` + `export_ship_glb`) end to end
    /// against a real ship, then checks the written file starts with the
    /// glTF Binary magic (`glTF`) and is non-empty. Same
    /// first-ship-from-the-real-catalog approach as
    /// `load_ship_armor_against_a_real_install_produces_a_renderable_model`.
    /// Run with:
    ///
    /// ```text
    /// WOWS_ARMOR_VIEWER_LOAD_TEST_DIR="E:\WoWs\World_of_Warships" \
    /// cargo test -p wows-toolkit-gpui -- --ignored export_ship_glb_against_a_real_install_writes_a_valid_glb_file
    /// ```
    #[test]
    #[ignore = "needs a local game install; see the doc comment for the run command"]
    fn export_ship_glb_against_a_real_install_writes_a_valid_glb_file() {
        let wows_dir = std::env::var("WOWS_ARMOR_VIEWER_LOAD_TEST_DIR")
            .expect("set WOWS_ARMOR_VIEWER_LOAD_TEST_DIR to a WoWs install directory");
        let wows_dir = std::path::PathBuf::from(wows_dir);

        let available =
            wowsunpack::game_data::list_available_builds(&wows_dir).expect("failed to list installed builds");
        let build = *available.last().expect("expected at least one installed build");

        let vfs = wowsunpack::game_data::build_game_vfs_for_build(&wows_dir, build)
            .expect("failed to build the game VFS for the latest installed build");
        let metadata = Arc::new(
            wowsunpack::game_params::provider::GameMetadataProvider::from_vfs(&vfs)
                .expect("failed to build GameMetadataProvider from the VFS"),
        );
        let ship_assets = ShipAssets::from_vfs_with_metadata(&vfs, Arc::clone(&metadata))
            .expect("failed to load ShipAssets from the VFS");

        let catalog = crate::armor_viewer::catalog::ShipCatalog::build(&metadata);
        let ship = catalog
            .nations
            .iter()
            .flat_map(|n| &n.classes)
            .flat_map(|c| &c.ships)
            .next()
            .expect("expected at least one ship in the real catalog");

        let out_dir = std::env::temp_dir().join("wows-toolkit-gpui-export-glb-test");
        std::fs::create_dir_all(&out_dir).expect("failed to create scratch output dir");
        let out_path = out_dir.join(default_export_filename(&ship.display_name));

        let options = export_options_from_selection(DEFAULT_LOD, None, HashMap::new());
        export_ship_glb(&ship_assets, &ship.param_index, &options, &out_path).unwrap_or_else(|e| {
            panic!("failed to export {} ({}) to {}: {e}", ship.display_name, ship.param_index, out_path.display())
        });

        let bytes = std::fs::read(&out_path).expect("failed to read back the exported glb file");
        assert!(!bytes.is_empty(), "expected a non-empty glb file");
        assert_eq!(&bytes[0..4], b"glTF", "expected the glTF Binary magic at the start of the file");

        println!(
            "exported {} ({}, build {build}) to {} ({} bytes)",
            ship.display_name,
            ship.param_index,
            out_path.display(),
            bytes.len()
        );
    }
}
