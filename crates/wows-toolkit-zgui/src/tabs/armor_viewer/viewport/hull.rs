//! Uploading the scene's meshes again as its settings change, and what the hull, camo and options
//! panels change: display settings, hull visibility and opacity, the camo worn, the hull upgrade,
//! level of detail and modules a reload is run with, the export and the lighting.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use rust_i18n::t;
use wows_toolkit_armor::camera_rings;
use wows_toolkit_armor::camo::build_active_camo;
use wows_toolkit_armor::gaps;
use wows_toolkit_armor::hits;
use wows_toolkit_armor::hull_picking;
use wows_toolkit_armor::length_marks::LengthMarks;
use wows_toolkit_armor::length_marks::MarkLabels;
use wows_toolkit_armor::load_model::ModelLoadRequest;
use wows_toolkit_armor::load_model::MountRef;
use wows_toolkit_armor::load_model::ProblemReason;
use wows_toolkit_armor::load_model::SceneLoadError;
use wows_toolkit_armor::load_model::SceneProblem;
use wows_toolkit_armor::load_model::load_model_scene;
use wows_toolkit_armor::load_ship;
use wows_toolkit_armor::load_ship::ExportGlbError;
use wows_toolkit_armor::load_ship::LoadedShipArmor;
use wows_toolkit_armor::load_ship::load_ship_armor_with_options;
use wows_toolkit_armor::plate_picking;
use wows_toolkit_armor::scene_load::FirstStage;
use wows_toolkit_armor::scene_load::RestJob;
use wows_toolkit_armor::scene_load::RestJobError;
use wows_toolkit_armor::scene_load::RestPiece;
use wows_toolkit_armor::splash;
use wows_toolkit_armor::upload;
use wows_toolkit_armor::upload_hull;
use wows_toolkit_armor::viewport::renderer::LAYER_OVERLAY;
use wows_toolkit_armor::viewport::types::LightingSettings;
use wows_toolkit_armor::visibility;
use wows_toolkit_armor::visibility::VisibilityFilter;
use wows_toolkit_armor::waterline_grid;
use wows_toolkit_services::dialog;
use wows_toolkit_viewmodel::formatting::byte_size;
use wowsunpack::export::camo_textures::CamoSchemeId;
use wowsunpack::export::camo_textures::CamoSchemeInfo;
use wowsunpack::export::camo_textures::SchemeTextures;
use wowsunpack::export::camouflage::UvTransform;
use wowsunpack::game_params::keys::ComponentType;
use zgui::reactive::blocking;
use zgui::reactive::spawn_detached;
use zgui::reactive::spawn_local;

use super::model::CamoTextures;
use super::model::ExportConfirm;
use super::model::ViewportModel;
use super::scene::first_camera_mode;
use super::scene::opening_camera;
use super::state::ArmorViewportState;
use super::state::ViewCx;
use super::types::SceneSource;
use super::types::ViewportChange;
use crate::shell::Shell;

/// Finds the scheme `old_id` named in `old_infos` in `new_infos`, by name. A `CamoSchemeId` is an
/// index into one load's scheme list, which a reload rebuilds, so the id alone can point at
/// another scheme or at nothing. `None` when `old_id` is unknown or no scheme of its name
/// survived the reload.
fn remap_camo_selection(
    old_infos: &[CamoSchemeInfo],
    new_infos: &[CamoSchemeInfo],
    old_id: CamoSchemeId,
) -> Option<CamoSchemeId> {
    let name = &old_infos.iter().find(|i| i.id == old_id)?.display_name;
    new_infos.iter().find(|i| &i.display_name == name).map(|i| i.id)
}

/// A reloaded scene, with the camo that was selected composited onto it and the background jobs
/// that finish it.
struct ReloadedScene {
    armor: LoadedShipArmor,
    /// `None` when no scheme was selected.
    camo: Option<ReloadedCamo>,
    rest: Vec<RestJob>,
}

/// The scheme selected before a reload, found again in the reloaded scene.
struct ReloadedCamo {
    was: CamoSchemeId,
    /// `None` when the reloaded scene has no scheme of that name.
    now: Option<CamoSchemeId>,
    decode: CamoDecode,
}

/// Finds `was` in `armor` by its name in `old_infos`, and composites it. Decodes and composites
/// full-size textures, so it runs off the UI thread.
fn reload_camo(was: CamoSchemeId, old_infos: &[CamoSchemeInfo], armor: &LoadedShipArmor) -> ReloadedCamo {
    let now = remap_camo_selection(old_infos, armor.camo_schemes(), was);
    let decode = match now {
        Some(id) => decode_active_camo(id, None, armor),
        None => CamoDecode { decoded: None, textures: HashMap::new(), uvs: HashMap::new() },
    };

    ReloadedCamo { was, now, decode }
}

/// One scheme's decoded and composited textures.
struct CamoDecode {
    /// The scheme's own textures, when this run decoded them; a run handed them from the cache
    /// leaves this `None`, since the caller already holds them.
    decoded: Option<Arc<SchemeTextures>>,
    /// Empty draws the base albedo only, which is what stock looks like.
    textures: CamoTextures,
    uvs: HashMap<String, UvTransform>,
}

/// Decodes `id`'s textures, unless `cached` already holds them, and composites them against
/// `armor`'s hull textures. A failed decode, or an id with no scheme entry, is logged and drawn
/// as stock rather than half applied. Takes no state, so it runs off the UI thread.
fn decode_active_camo(id: CamoSchemeId, cached: Option<Arc<SchemeTextures>>, armor: &LoadedShipArmor) -> CamoDecode {
    let (textures, decoded) = match cached {
        Some(t) => (t, None),
        None => match armor.camo_texture_source().map(|source| source.decode(id)) {
            None => {
                tracing::warn!("camo scheme {id:?} asked of a model with no camouflage; rendering as stock");
                return CamoDecode { decoded: None, textures: HashMap::new(), uvs: HashMap::new() };
            }
            Some(Ok(t)) => {
                if t.is_empty() {
                    tracing::warn!("camo scheme {id:?} decoded to zero textures for this ship; rendering as stock");
                }
                let t = Arc::new(t);
                (Arc::clone(&t), Some(t))
            }
            Some(Err(e)) => {
                tracing::warn!("failed to decode camo scheme {id:?}: {e}");
                return CamoDecode { decoded: None, textures: HashMap::new(), uvs: HashMap::new() };
            }
        },
    };
    let info = armor.camo_schemes().iter().find(|i| i.id == id);
    let (uv, use_color_scheme) = match info {
        Some(i) => (i.uv_transforms.clone(), i.use_color_scheme),
        None => {
            tracing::warn!(
                "camo scheme {id:?} decoded successfully but has no matching entry in camo_scheme_infos; falling back to identity UVs and no color scheme"
            );
            (HashMap::default(), false)
        }
    };
    let (built, uvs) = build_active_camo(&textures, &uv, use_color_scheme, &armor.hull_textures);
    CamoDecode { decoded, textures: built, uvs }
}

/// Reports a finished export: what was written and how large it is, or why it was not. The
/// destination is logged rather than shown: the reader chose it in the save dialog.
fn report_export(shell: Shell, written: Result<(PathBuf, Option<u64>), ExportGlbError>, ship: &str) {
    // The window can close while the file is written.
    let toasts = shell.try_toasts();
    match written {
        Ok((path, size)) => {
            tracing::info!("armor viewer: exported {ship} to {}", path.display());
            let message = match size {
                Some(bytes) => {
                    t!("ui.armor.export.exported_with_size", ship = ship, size = byte_size(bytes)).into_owned()
                }
                None => t!("ui.armor.export.exported", ship = ship).into_owned(),
            };
            if let Some(toasts) = toasts {
                toasts.ok(message);
            }
        }
        Err(err) => {
            tracing::error!("armor viewer: failed to export {ship}: {err}");
            if let Some(toasts) = toasts {
                toasts.failed(t!("ui.armor.export.export_failed", error = err.to_string()).into_owned());
            }
        }
    }
}

impl ViewportModel {
    /// Uploads the shown scene again under the current visibility and settings, without moving
    /// the camera. A hovered plate that is still drawn keeps its highlight, rebuilt on the new
    /// geometry; one just hidden loses its hover at once.
    pub(super) fn reupload_current_armor(&mut self, cx: &mut ViewCx) {
        let Some(scene) = self.scene() else { return };
        let Some(armor) = self.current_armor.clone() else { return };
        let device = scene.device();
        let visibility = VisibilityFilter {
            part: &self.part_visibility,
            plate: &self.plate_visibility,
            show_hidden_only: self.show_hidden_only,
        };
        let display = armor.effective_display(self.display_settings);
        self.mesh_triangle_info =
            upload::reupload_armor_plates(&mut self.viewport, device, &armor, visibility, display);
        // The armor upload cleared every mesh, the hull's included.
        let hull_opaque = self.hull_opaque();
        upload_hull::upload_hull_meshes(
            &mut self.viewport,
            device,
            scene.queue(),
            scene.pipeline(),
            &armor,
            &mut self.hull_mesh_ids,
            &self.hull_visibility,
            hull_opaque,
            &self.active_camo_textures,
            &self.active_camo_uvs,
            &mut self.hull_textures,
        );
        self.hull_shown = hull_picking::shown_meshes(&armor.hull_meshes, &self.hull_visibility);
        self.ring_hovers.clear();
        if self.camera_rings.shown
            && let Some((mode, trajectory)) =
                armor.camera_trajectories.iter().find(|(name, _)| Some(name) == self.camera_rings.mode.as_ref())
        {
            let request = camera_rings::RingRequest {
                mode,
                trajectory,
                fov: self.camera_rings.fov,
                height: self.camera_rings.height,
                zoom_path: self.camera_rings.zoom_path,
                zoom_path_at_fov: self.camera_rings.zoom_path_at_fov,
                zoom_path_at_max_fov: self.camera_rings.zoom_path_at_max_fov,
                // The orbits sit at the height the trajectory states: the armor carries no
                // waterline offset.
                waterline_dy: 0.0,
            };
            let (vertices, indices, hovers) = camera_rings::build_camera_rings(&request);
            if !indices.is_empty() {
                self.viewport.add_non_pickable_mesh(device, &vertices, &indices, LAYER_OVERLAY);
            }
            self.ring_hovers = hovers;
        }

        // A model has no water to stand in, so its grid lies under it.
        let plane_y = if armor.is_ship() { 0. } else { armor.bounds.0.y };
        let marks = LengthMarks::new(armor.bounds, plane_y);
        if self.length_marks.as_ref().map(MarkLabels::marks) != marks.as_ref() {
            self.length_marks = marks.map(MarkLabels::new);
        }
        let layers = waterline_grid::GridLayers {
            lines: self.display_settings.show_grid,
            marks: self.display_settings.show_length_marks,
        };
        if let Some(written) = self.length_marks.as_ref() {
            waterline_grid::upload_waterline_grid(&mut self.viewport, device, written.marks(), layers);
        }

        if self.display_settings.show_ship_center {
            let (vertices, indices) = hits::build_center_marker(armor.bounds);
            self.viewport.add_non_pickable_mesh(device, &vertices, &indices, LAYER_OVERLAY);
        }

        if !self.hits.is_empty() {
            let (vertices, indices) = hits::build_markers(
                &self.hits,
                armor.center(),
                Some(armor.bounds),
                self.display_settings.marker_opacity,
            );
            if !indices.is_empty() {
                self.viewport.add_non_pickable_mesh(device, &vertices, &indices, LAYER_OVERLAY);
            }
        }

        if let Some(result) = self.splash_result.as_ref() {
            let (vertices, indices) =
                splash::build_splash_cube_mesh(result.impact_point, result.half_extent, splash::CUBE_COLOR);
            if !indices.is_empty() {
                self.viewport.add_non_pickable_mesh(device, &vertices, &indices, LAYER_OVERLAY);
            }
        }

        // Walked over the same triangles the armor pass just uploaded, so a hidden plate's rim is
        // not reported as a hole in the ship.
        self.gap_count = 0;
        if self.show_gaps {
            let (vertices, indices, count) = gaps::build_gap_mesh(&armor, visibility, display.show_zero_mm);
            self.gap_count = count;
            if !indices.is_empty() {
                self.viewport.add_non_pickable_mesh(device, &vertices, &indices, LAYER_OVERLAY);
            }
        }
        // After the passes that read the visibility filter, which borrows this viewport while
        // these want it back.
        self.apply_perspective(&scene, &armor);
        self.upload_splash_boxes(&scene, &armor);
        self.upload_hardpoints();

        // The upload dropped the old highlight meshes; their ids are stale.
        self.hover_highlight = None;
        if let Some(hover) = self.hovered.clone() {
            let still_visible = self
                .mesh_triangle_info
                .iter()
                .flat_map(|(_, tooltips)| tooltips)
                .any(|t| plate_picking::plate_key_of(t) == hover.key);
            if still_visible {
                self.rebuild_hover_highlight(&hover.key);
            } else {
                self.hovered = None;
            }
        }
        if let Some((key, _)) = self.sidebar_highlight.take() {
            self.rebuild_sidebar_highlight(key);
        }
        self.mesh_highlight = None;
        self.revalidate_mesh_hover();
        self.viewport.mark_dirty();
        cx.notify();
    }

    /// Changes the display settings and uploads the armor again: they shape its geometry.
    pub(super) fn mutate_display_settings(
        &mut self,
        cx: &mut ViewCx,
        mutate: impl FnOnce(&mut upload::DisplaySettings),
    ) {
        mutate(&mut self.display_settings);
        self.reupload_current_armor(cx);
        cx.emit(ViewportChange::Settings);
    }

    /// Uploads the hull meshes again under the current hull visibility and opacity. Leaves the
    /// armor, the visibility and the camera alone, so a hull-only change stays cheap. A no-op
    /// without a device or a scene.
    pub(super) fn reupload_hull(&mut self) {
        let Some(scene) = self.scene() else { return };
        let Some(armor) = self.current_armor.clone() else { return };
        let hull_opaque = self.hull_opaque();
        upload_hull::upload_hull_meshes(
            &mut self.viewport,
            scene.device(),
            scene.queue(),
            scene.pipeline(),
            &armor,
            &mut self.hull_mesh_ids,
            &self.hull_visibility,
            hull_opaque,
            &self.active_camo_textures,
            &self.active_camo_uvs,
            &mut self.hull_textures,
        );
        self.hull_shown = hull_picking::shown_meshes(&armor.hull_meshes, &self.hull_visibility);
        self.revalidate_mesh_hover();
    }

    /// Changes the hull visibility and uploads only the hull again.
    pub(super) fn mutate_hull_visibility(&mut self, cx: &mut ViewCx, mutate: impl FnOnce(&mut HashMap<String, bool>)) {
        mutate(&mut self.hull_visibility);
        self.mesh_seed.hull = false;
        self.reupload_hull();
        cx.emit(ViewportChange::Settings);
        cx.notify();
    }

    /// Whether the hull meshes on screen are drawn opaque: the saved setting for a ship, this
    /// pane's own choice for a standalone model.
    pub(super) fn hull_opaque(&self) -> bool {
        if self.shown().is_some_and(|armor| !armor.is_ship()) {
            self.model_hull_opaque
        } else {
            self.display_settings.hull_opaque
        }
    }

    /// Sets [`Self::hull_opaque`] and uploads only the hull again, whose alpha and depth layer
    /// depend on it.
    pub(super) fn set_hull_opaque(&mut self, opaque: bool, cx: &mut ViewCx) {
        if self.hull_opaque() == opaque {
            return;
        }
        if self.shown().is_some_and(|armor| !armor.is_ship()) {
            self.model_hull_opaque = opaque;
        } else {
            self.display_settings.hull_opaque = opaque;
        }
        self.reupload_hull();
        cx.emit(ViewportChange::Settings);
        cx.notify();
    }

    /// Selects the camo scheme worn, `None` for stock, and uploads only the hull again. The scheme
    /// is decoded and composited off the UI thread; the hull keeps the camo it has until the new
    /// one lands.
    pub(super) fn select_camo(&mut self, id: Option<CamoSchemeId>, cx: &mut ViewCx) {
        if self.selected_camo == id {
            return;
        }
        self.selected_camo = id;
        self.camo_generation = self.camo_generation.wrapping_add(1);

        let Some((id, armor)) = id.zip(self.current_armor.clone()) else {
            self.active_camo_textures.clear();
            self.active_camo_uvs.clear();
            self.hull_textures.forget_camo();
            self.reupload_hull();
            cx.emit(ViewportChange::Settings);
            cx.notify();
            return;
        };

        let generation = self.camo_generation;
        let cached = self.camo_texture_cache.get(id);
        cx.defer(move |state| {
            spawn_local(async move {
                let decode = blocking(move || decode_active_camo(id, cached, &armor)).await;
                state.update(|this, cx| this.apply_camo_decode(generation, id, decode, cx));
            });
        });
        cx.notify();
    }

    /// Takes a finished camo decode, unless the selection moved on while it ran.
    fn apply_camo_decode(&mut self, generation: u64, id: CamoSchemeId, decode: CamoDecode, cx: &mut ViewCx) {
        if generation != self.camo_generation {
            return;
        }
        if let Some(textures) = decode.decoded {
            self.camo_texture_cache.insert(id, textures);
        }
        self.active_camo_textures = decode.textures;
        self.active_camo_uvs = decode.uvs;
        self.hull_textures.forget_camo();
        self.reupload_hull();
        cx.emit(ViewportChange::Settings);
        cx.notify();
    }

    /// The hull this viewport is showing, for an export seeded from it.
    pub(super) fn selected_hull(&self) -> Option<String> {
        self.selected_hull.clone()
    }

    /// Switches to `key`'s hull upgrade and reloads. The module alternatives differ per hull, so
    /// the ones chosen are dropped.
    pub(super) fn select_hull_upgrade(&mut self, key: String, cx: &mut ViewCx) {
        if self.selected_hull.as_ref() == Some(&key) {
            return;
        }
        self.selected_hull = Some(key);
        self.selected_modules.clear();
        self.reload_ship(cx);
    }

    /// Switches to the level of detail `lod` and reloads.
    pub(super) fn select_hull_lod(&mut self, lod: usize, cx: &mut ViewCx) {
        if self.hull_lod == lod {
            return;
        }
        self.hull_lod = lod;
        self.reload_ship(cx);
    }

    /// Fits `name` as the `ct` component and reloads.
    pub(super) fn select_module_alternative(&mut self, ct: ComponentType, name: String, cx: &mut ViewCx) {
        if self.selected_modules.get(&ct) == Some(&name) {
            return;
        }
        self.selected_modules.insert(ct, name);
        self.reload_ship(cx);
    }

    /// Loads the scene shown again off the UI thread under the selected hull, level of detail and
    /// modules. A loaded scene is never changed in place, so every selector reloads it whole. A
    /// no-op before the first load.
    pub(super) fn reload_ship(&mut self, cx: &mut ViewCx) {
        let Some(source) = self.scene_source.clone() else { return };
        let cancel = self.reload_load.start();
        type Load = Box<dyn FnOnce() -> Result<FirstStage, SceneLoadError> + Send>;
        let load: Load = match source {
            SceneSource::Ship { assets, param_index, display_name } => {
                let options = match load_ship::build_reload_options(
                    &assets,
                    &param_index,
                    display_name,
                    self.hull_lod,
                    self.selected_hull.clone(),
                    self.selected_modules.clone(),
                ) {
                    Ok(options) => options,
                    Err(e) => {
                        tracing::error!("armor viewer: failed to build reload options: {e}");
                        return;
                    }
                };
                let cancel = cancel.clone();
                Box::new(move || {
                    load_ship_armor_with_options(&assets, &param_index, options, &cancel).map_err(SceneLoadError::from)
                })
            }
            SceneSource::Model { assets, models, request } => {
                let request = ModelLoadRequest { lod: self.hull_lod, ..request };
                let cancel = cancel.clone();
                Box::new(move || {
                    load_model_scene(&models, assets.metadata(), &request, &cancel).map_err(SceneLoadError::from)
                })
            }
        };

        // The scheme is carried by name across the reload and composited with it, off the UI
        // thread, so the reloaded hull is never shown stock first. The reloaded scheme list is
        // read before the reload lands rather than left to the background stage, so a scheme
        // picked meanwhile is found in it too.
        let shown_schemes = self.current_armor.as_ref().map(|armor| armor.camo_schemes().to_vec()).unwrap_or_default();
        let camo = self.selected_camo.map(|id| (id, shown_schemes));
        let work = move || -> Result<ReloadedScene, SceneLoadError> {
            let FirstStage { scene, mut rest } = load()?;
            if let Some(at) = rest.iter().position(|job| job.piece() == RestPiece::Camo) {
                match rest.remove(at).run(&scene, &cancel) {
                    Ok(()) => {}
                    Err(RestJobError::Cancelled(cancelled)) => return Err(SceneLoadError::from(cancelled)),
                    Err(e) => tracing::warn!("armor viewer: {} lacks its camouflage: {e}", scene.display_name),
                }
            }
            let camo = camo.map(|(was, infos)| reload_camo(was, &infos, &scene));

            Ok(ReloadedScene { armor: scene, camo, rest })
        };

        self.reload_generation += 1;
        let generation = self.reload_generation;
        let camo_generation = self.camo_generation;
        cx.defer(move |state| {
            spawn_local(async move {
                let result = blocking(work).await;
                state.update(|this, cx| this.apply_reload_result(generation, camo_generation, result, cx));
            });
        });
    }

    /// Reads a model's armor thickness from another ship mounting it, keeping the view as it is.
    pub(super) fn set_thickness_source(&mut self, mount: MountRef, cx: &mut ViewCx) {
        let Some(SceneSource::Model { request, .. }) = &mut self.scene_source else { return };
        if request.thickness_from.as_ref() == Some(&mount) {
            return;
        }
        request.thickness_from = Some(mount);
        self.reload_ship(cx);
    }

    /// Opens the export-confirm panel for the ship shown, fixing its target and the hull, level
    /// of detail and modules selected now, so a later selection does not retarget it. A model has
    /// no hull or camo to confirm: its request is handed back for the pane's export dialog, which
    /// asks what the file should contain.
    pub(super) fn open_export_confirm(&mut self, cx: &mut ViewCx) -> Option<ModelLoadRequest> {
        self.current_armor.as_ref()?;
        let (assets, param_index, display_name) = match &self.scene_source {
            Some(SceneSource::Ship { assets, param_index, display_name }) => (assets, param_index, display_name),
            Some(SceneSource::Model { request, .. }) => return Some(request.clone()),
            None => return None,
        };
        self.export_confirm = Some(ExportConfirm {
            assets: Arc::clone(assets),
            param_index: param_index.clone(),
            display_name: display_name.clone(),
            hull_lod: self.hull_lod,
            selected_hull: self.selected_hull.clone(),
            selected_modules: self.selected_modules.clone(),
        });
        cx.notify();
        None
    }

    /// Closes the export-confirm panel without exporting.
    pub(super) fn cancel_export(&mut self, cx: &mut ViewCx) {
        self.export_confirm = None;
        cx.notify();
    }

    /// Closes the export-confirm panel and asks where to write the file. On a chosen path the
    /// ship is exported off the UI thread as the panel fixed it when it opened, and the outcome
    /// is reported as a message.
    pub(super) fn confirm_export(&mut self, shell: Shell, cx: &mut ViewCx) {
        let Some(snapshot) = self.export_confirm.take() else { return };
        cx.notify();
        let ExportConfirm { assets, param_index, display_name, hull_lod, selected_hull, selected_modules } = snapshot;
        let options = load_ship::export_options_from_selection(hull_lod, selected_hull, selected_modules);

        cx.defer(move |_state| {
            let asked = dialog::save_file(None, &load_ship::default_export_filename(&display_name), Some(dialog::GLB));
            // Detached: a file the reader asked for is written whatever becomes of the viewport.
            spawn_detached(async move {
                let Some(path) = shell.chosen_path(asked).await else { return };
                let written = blocking(move || {
                    let result = load_ship::export_ship_glb(&assets, &param_index, &options, &path);
                    // The size is only knowable once the file is on disk.
                    result.map(|()| {
                        let size = std::fs::metadata(&path).map(|meta| meta.len()).ok();
                        (path, size)
                    })
                })
                .await;
                report_export(shell, written, &display_name);
            });
        });
    }

    /// Takes a finished reload, unless a later one superseded it. The camo is carried over by
    /// name and the visibility entries that still apply are kept. The camera stays where the
    /// reader left it; the undo history and every hover go, since they can name parts the reload
    /// removed.
    fn apply_reload_result(
        &mut self,
        generation: u64,
        camo_generation: u64,
        result: Result<ReloadedScene, SceneLoadError>,
        cx: &mut ViewCx,
    ) {
        if generation != self.reload_generation {
            return;
        }
        let ReloadedScene { armor, camo, rest } = match result {
            Ok(reloaded) => reloaded,
            Err(e) if e.is_cancelled() => return,
            Err(e) => {
                tracing::error!("armor viewer: failed to reload: {e}");
                if let Some(shown) = &self.current_armor {
                    let problem = SceneProblem { subject: shown.display_name.clone().into(), reason: e.problem() };
                    self.show_problem(problem, cx);
                }
                return;
            }
        };
        let armor = Arc::new(armor);

        // A scheme picked while the reload ran replaces the one it carried, and is found by name
        // in the reloaded scene once that is shown.
        let picked_since = (camo_generation != self.camo_generation).then(|| {
            let old_infos: &[CamoSchemeInfo] = self.current_armor.as_ref().map(|old| old.camo_schemes()).unwrap_or(&[]);
            self.selected_camo.and_then(|id| remap_camo_selection(old_infos, armor.camo_schemes(), id))
        });
        // Keyed by the ids before the reload, which no longer index the reloaded scheme list.
        self.camo_texture_cache.clear();
        let (selected, textures, uvs) = match camo.filter(|_| picked_since.is_none()) {
            None => (None, HashMap::new(), HashMap::new()),
            Some(ReloadedCamo { was, now, decode }) => {
                if now.is_none() {
                    tracing::warn!(
                        "armor viewer: selected camo scheme {was:?} did not survive the reload (name not found in the reloaded ship's scheme list); reverting to stock"
                    );
                }
                if let (Some(id), Some(decoded)) = (now, decode.decoded) {
                    self.camo_texture_cache.insert(id, decoded);
                }
                (now, decode.textures, decode.uvs)
            }
        };
        self.selected_camo = selected;
        self.camo_generation = self.camo_generation.wrapping_add(1);
        // A reload can change the hull, and with it which modes are named.
        if !armor.camera_trajectories.iter().any(|(name, _)| Some(name) == self.camera_rings.mode.as_ref()) {
            self.camera_rings.mode = first_camera_mode(&armor);
        }

        self.reseed_mesh(&armor);
        visibility::retain_hull_visibility(&mut self.hull_visibility, &armor.hull_part_groups);
        visibility::retain_part_visibility(&mut self.part_visibility, &armor.zone_parts);
        visibility::retain_plate_visibility(&mut self.plate_visibility, &armor.zone_part_plates);
        self.undo_stack.clear();
        self.hovered = None;
        self.hover_highlight = None;
        self.hovered_ring = None;
        self.ring_hovers.clear();
        self.sidebar_highlight = None;
        self.hovered_mesh = None;
        self.mesh_highlight = None;
        self.hull_shown.clear();

        self.active_camo_textures = textures;
        self.active_camo_uvs = uvs;
        self.model_bounds = Some(armor.bounds);
        let (width, height) = self.viewport_size();
        self.home_camera = Some(opening_camera(&armor, self.shown_layers(&armor), width / height.max(1.0)));
        self.hull_textures.clear();
        self.scene_problem = armor.emptiness().map(|empty| {
            tracing::warn!("armor viewer: {} has nothing to draw: {empty:?}", armor.display_name);
            SceneProblem { subject: armor.display_name.clone().into(), reason: ProblemReason::Empty(empty) }
        });
        self.current_armor = Some(Arc::clone(&armor));
        self.reupload_current_armor(cx);
        self.start_scene_rest(armor, rest, cx);
        if let Some(picked) = picked_since {
            self.select_camo(picked, cx);
        }
    }

    /// Expands or collapses a camo-origin group in the camo picker.
    pub(super) fn toggle_camo_group_expanded(&mut self, group: String, cx: &mut ViewCx) {
        if !self.expanded_camo_groups.remove(&group) {
            self.expanded_camo_groups.insert(group);
        }
        cx.notify();
    }

    pub(super) fn lighting(&self) -> LightingSettings {
        self.viewport.lighting.clone()
    }

    /// Changes the lighting. It is a per-frame uniform, so no mesh is uploaded again.
    pub(super) fn mutate_lighting(&mut self, cx: &mut ViewCx, mutate: impl FnOnce(&mut LightingSettings)) {
        mutate(&mut self.viewport.lighting);
        self.viewport.mark_dirty();
        cx.emit(ViewportChange::Settings);
        cx.notify();
    }

    /// Replaces the lighting with `preset`, keeping whether lighting is on.
    pub(super) fn set_lighting_preset(&mut self, mut preset: LightingSettings, cx: &mut ViewCx) {
        preset.enabled = self.viewport.lighting.enabled;
        self.viewport.lighting = preset;
        self.viewport.mark_dirty();
        cx.emit(ViewportChange::Settings);
        cx.notify();
    }
}

impl ArmorViewportState {
    pub fn display_settings(self) -> upload::DisplaySettings {
        self.read(|this| this.display_settings).unwrap_or_default()
    }

    pub fn mutate_display_settings(self, mutate: impl FnOnce(&mut upload::DisplaySettings)) {
        self.update(|this, cx| this.mutate_display_settings(cx, mutate));
    }

    pub fn mutate_hull_visibility(self, mutate: impl FnOnce(&mut HashMap<String, bool>)) {
        self.update(|this, cx| this.mutate_hull_visibility(cx, mutate));
    }

    pub fn hull_opaque(self) -> bool {
        self.read(|this| this.hull_opaque()).unwrap_or_default()
    }

    pub fn set_hull_opaque(self, opaque: bool) {
        self.update(|this, cx| this.set_hull_opaque(opaque, cx));
    }

    pub fn select_camo(self, id: Option<CamoSchemeId>) {
        self.update(|this, cx| this.select_camo(id, cx));
    }

    pub fn selected_hull(self) -> Option<String> {
        self.read(|this| this.selected_hull()).flatten()
    }

    pub fn select_hull_upgrade(self, key: String) {
        self.update(|this, cx| this.select_hull_upgrade(key, cx));
    }

    pub fn select_hull_lod(self, lod: usize) {
        self.update(|this, cx| this.select_hull_lod(lod, cx));
    }

    pub fn select_module_alternative(self, ct: ComponentType, name: String) {
        self.update(|this, cx| this.select_module_alternative(ct, name, cx));
    }

    pub fn set_thickness_source(self, mount: MountRef) {
        self.update(|this, cx| this.set_thickness_source(mount, cx));
    }

    /// Opens the export-confirm panel for a ship. For a model, hands back its request for the
    /// pane's export dialog instead.
    pub fn open_export_confirm(self) -> Option<ModelLoadRequest> {
        self.update(|this, cx| this.open_export_confirm(cx)).flatten()
    }

    pub fn cancel_export(self) {
        self.update(|this, cx| this.cancel_export(cx));
    }

    /// Exports the ship the confirm panel fixed, asking where through `shell`'s file dialog and
    /// reporting through its messages.
    pub fn confirm_export(self, shell: Shell) {
        self.update(|this, cx| this.confirm_export(shell, cx));
    }

    pub fn toggle_camo_group_expanded(self, group: String) {
        self.update(|this, cx| this.toggle_camo_group_expanded(group, cx));
    }

    pub fn lighting(self) -> LightingSettings {
        self.read(|this| this.lighting()).unwrap_or_default()
    }

    pub fn mutate_lighting(self, mutate: impl FnOnce(&mut LightingSettings)) {
        self.update(|this, cx| this.mutate_lighting(cx, mutate));
    }

    pub fn set_lighting_preset(self, preset: LightingSettings) {
        self.update(|this, cx| this.set_lighting_preset(preset, cx));
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use wows_toolkit_armor::load_model::fixtures::model_scene;
    use wowsunpack::export::camo_textures::CamoSchemeId;
    use wowsunpack::export::camo_textures::CamoSchemeInfo;
    use wowsunpack::export::gltf_export::CamoOrigin;
    use wowsunpack::export::model_catalog::ModelPath;

    use super::reload_camo;
    use super::remap_camo_selection;

    fn scheme(id: usize, display_name: &str, origin: CamoOrigin) -> CamoSchemeInfo {
        CamoSchemeInfo {
            id: CamoSchemeId(id),
            display_name: display_name.to_string(),
            raw_name: display_name.to_string(),
            origin,
            use_color_scheme: false,
            uv_transforms: HashMap::new(),
        }
    }

    #[test]
    fn remap_camo_selection_follows_name_across_shuffled_ids() {
        let old = vec![scheme(0, "Camo A", CamoOrigin::ShipSpecific), scheme(1, "Camo B", CamoOrigin::Universal)];
        // The reload rebuilt the scheme list in another order: "Camo A" is now id 1.
        let new = vec![scheme(0, "Camo B", CamoOrigin::Universal), scheme(1, "Camo A", CamoOrigin::ShipSpecific)];
        assert_eq!(remap_camo_selection(&old, &new, CamoSchemeId(0)), Some(CamoSchemeId(1)));
    }

    #[test]
    fn remap_camo_selection_none_when_name_gone_from_new_list() {
        let old = vec![scheme(0, "Camo A", CamoOrigin::ShipSpecific)];
        let new: Vec<CamoSchemeInfo> = vec![scheme(0, "Camo B", CamoOrigin::Universal)];
        assert_eq!(remap_camo_selection(&old, &new, CamoSchemeId(0)), None);
    }

    #[test]
    fn remap_camo_selection_none_when_old_id_unknown() {
        let old: Vec<CamoSchemeInfo> = vec![scheme(0, "Camo A", CamoOrigin::ShipSpecific)];
        let new = vec![scheme(0, "Camo A", CamoOrigin::ShipSpecific)];
        assert_eq!(remap_camo_selection(&old, &new, CamoSchemeId(5)), None);
    }

    /// The reload finds the selected scheme by name in the reloaded scene, on the thread that
    /// loaded it.
    #[test]
    fn a_reload_carries_the_selected_camo_by_name() {
        let old = vec![scheme(0, "Camo A", CamoOrigin::ShipSpecific), scheme(1, "Camo B", CamoOrigin::Universal)];
        let path = ModelPath::parse("content/gameplay/usa/gun/main/AGM034/AGM034.model").expect("a model path");
        let mut armor = model_scene(path, Vec::new());
        armor.camo_scheme_infos =
            vec![scheme(0, "Camo B", CamoOrigin::Universal), scheme(1, "Camo A", CamoOrigin::ShipSpecific)].into();

        let reloaded = reload_camo(CamoSchemeId(0), &old, &armor);

        assert_eq!((reloaded.was, reloaded.now), (CamoSchemeId(0), Some(CamoSchemeId(1))));
        assert!(reload_camo(CamoSchemeId(5), &old, &armor).now.is_none(), "an unknown scheme reverts to stock");
    }
}
