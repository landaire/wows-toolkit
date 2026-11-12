//! The scene a viewport shows: handing it over and uploading it, the loads that bring it, what is
//! read off it for the panels, the settings mirrored between panes, the camera lock and orbits,
//! and the hardpoint markers.

use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;

use wows_toolkit_armor::camera_rings;
use wows_toolkit_armor::hardpoint_overlay::HardpointLabel;
use wows_toolkit_armor::hardpoint_overlay::HardpointMarkers;
use wows_toolkit_armor::load_model::SceneProblem;
use wows_toolkit_armor::load_ship;
use wows_toolkit_armor::load_ship::LoadedShipArmor;
use wows_toolkit_armor::load_ship::SceneSubject;
use wows_toolkit_armor::model_refs::HardpointTarget;
use wows_toolkit_armor::model_refs::OpenTarget;
use wows_toolkit_armor::scene_load::LoadCancel;
use wows_toolkit_armor::scene_load::RestJob;
use wows_toolkit_armor::scene_load::RestJobError;
use wows_toolkit_armor::scene_load::RestPiece;
use wows_toolkit_armor::scene_load::SharedLoad;
use wows_toolkit_armor::splash;
use wows_toolkit_armor::upload;
use wows_toolkit_armor::viewport::camera::ArcballCamera;
use wows_toolkit_armor::viewport::renderer::LAYER_OVERLAY;
use wows_toolkit_armor::viewport::types::Vec3;
use wows_toolkit_config::queries::ArmorViewerDefaultsRow;
use wows_toolkit_viewmodel::armor::camera_perspective::LookMode;
use wows_toolkit_viewmodel::armor::camera_perspective::water_aim_point;
use wowsunpack::export::ship::ShipAssets;
use wowsunpack::game_params::ttx::armor_material::ArmorMaterial;
use wowsunpack::game_params::types::CameraTrajectory;
use wowsunpack::game_params::types::Species;
use zgui::reactive::blocking;
use zgui::reactive::spawn_local;

use super::model::SceneDevice;
use super::model::ViewportModel;
use super::state::ArmorViewportState;
use super::state::ViewCx;
use super::types::AllVisible;
use super::types::CameraRingSettings;
use super::types::LoadGeneration;
use super::types::MeshSeed;
use super::types::OpeningLayer;
use super::types::OpeningLoad;
use super::types::PerspectiveSettings;
use super::types::SceneFeatures;
use super::types::SceneSource;
use super::types::SyncedSettings;

/// How far the locked camera's aim marker reaches, as a fraction of the model's diagonal, and the
/// span it is held to so it is legible on any ship.
const AIM_MARKER_FRACTION: f32 = 0.03;
const AIM_MARKER_MIN: f32 = 1.0;
const AIM_MARKER_MAX: f32 = 8.0;

/// What the locked camera's aim marker is coloured.
const AIM_MARKER_COLOR: [f32; 4] = [1.0, 0.6, 0.1, 0.85];

/// How far along the look direction the water is searched for, so a camera aimed near the
/// horizon marks a far point rather than none.
const AIM_MARKER_REACH: f32 = 5000.0;

/// What a drag and the wheel do to the locked camera, per pixel and per notch.
const PERSPECTIVE_YAW_PER_PX: f32 = 0.005;
const PERSPECTIVE_PITCH_PER_PX: f32 = 0.005;
const PERSPECTIVE_ZOOM_PER_NOTCH: f32 = 0.001;

/// The mode a ship opens on: its first, which is the one the game itself uses. `None` for a ship
/// whose GameParams name none, where there is nothing to draw.
pub(super) fn first_camera_mode(armor: &LoadedShipArmor) -> Option<String> {
    armor.camera_trajectories.first().map(|(name, _)| name.clone())
}

/// The visibility a newly opened model starts with.
///
/// An absent part is shown and an absent hull mesh is hidden, so only the other outcome gets
/// entries.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct OpeningVisibility {
    pub(super) parts: HashMap<(String, String), bool>,
    pub(super) hull: HashMap<String, bool>,
}

impl OpeningVisibility {
    pub(super) fn new(armor: &LoadedShipArmor, saved: AllVisible, layer: OpeningLayer) -> Self {
        Self::from_groups(&armor.zone_parts, &armor.hull_part_groups, saved, layer)
    }

    fn from_groups(
        zone_parts: &[(String, Vec<String>)],
        hull_part_groups: &[(String, Vec<String>)],
        saved: AllVisible,
        layer: OpeningLayer,
    ) -> Self {
        let mut opening = Self::default();
        if !saved.armor || layer == OpeningLayer::Mesh {
            for (zone, parts) in zone_parts {
                for part in parts {
                    opening.parts.insert((zone.clone(), part.clone()), false);
                }
            }
        }
        if saved.hull || layer == OpeningLayer::Mesh {
            for (_group, names) in hull_part_groups {
                for name in names {
                    opening.hull.insert(name.clone(), true);
                }
            }
        }
        opening
    }
}

/// Which of a scene's layers are drawn when it opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ShownLayers {
    pub(super) armor: bool,
    pub(super) hull: bool,
}

/// Where the camera starts on `armor`, in a view `aspect` wide per unit of height.
///
/// A ship is framed on its bounds. A model is framed on the layers drawn, so a hidden layer that
/// reaches further does not push the model shown into the distance.
pub(super) fn opening_camera(armor: &LoadedShipArmor, shown: ShownLayers, aspect: f32) -> ArcballCamera {
    if armor.is_ship() {
        let (min, max) = armor.bounds;
        return ArcballCamera::from_bounds(min, max);
    }

    let plates: &[_] = if shown.armor { &armor.meshes } else { &[] };
    let hull: &[_] = if shown.hull { &armor.hull_meshes } else { &[] };
    let (min, max) = load_ship::scene_bounds(plates, hull);
    // Nothing drawn has any extent, so the whole scene stands in for it.
    let (min, max) = if min.x <= max.x { (min, max) } else { armor.bounds };

    ArcballCamera::framing(min, max, aspect)
}

impl ViewportModel {
    /// Seeds the display settings and the visibility a new ship opens with from the saved
    /// defaults, the built-in defaults when there are none, and uploads the shown scene again.
    pub(super) fn apply_armor_defaults(&mut self, defaults: Option<&ArmorViewerDefaultsRow>, cx: &mut ViewCx) {
        self.display_settings = upload::DisplaySettings::from_defaults(defaults);
        self.armor_all_visible = defaults.map(|d| d.armor_all_visible).unwrap_or(true);
        self.hull_all_visible = defaults.map(|d| d.hull_all_visible).unwrap_or(false);
        self.reupload_current_armor(cx);
        cx.notify();
    }

    /// The assets this pane's scene was loaded from, which is where the translations for part and
    /// material names come from.
    pub(super) fn reload_assets(&self) -> Option<Arc<ShipAssets>> {
        self.scene_source.as_ref().map(|source| Arc::clone(source.assets()))
    }

    /// The class of the ship shown, which picks the game's class-specific plate names.
    pub(super) fn shown_species(&self) -> Option<Species> {
        self.current_armor.as_ref().and_then(|armor| armor.species)
    }

    /// `material` named the way the game names it on the ship shown, or by its English label
    /// while the build is still loading.
    pub(super) fn material_label(&self, material: ArmorMaterial) -> String {
        match self.reload_assets() {
            Some(assets) => material.localized_name(assets.metadata(), self.shown_species()),
            None => material.english_label(),
        }
    }

    pub(super) fn set_scene_source(&mut self, source: SceneSource) {
        self.scene_source = Some(source);
    }

    /// The scene last handed to this pane: the one waiting for the device, which is newer than the
    /// one on screen, or else the one on screen.
    pub(super) fn shown(&self) -> Option<&Arc<LoadedShipArmor>> {
        self.pending_armor.as_ref().map(|(armor, _)| armor).or(self.current_armor.as_ref())
    }

    /// Which of `armor`'s layers the visibility maps draw any of. An absent part is drawn and an
    /// absent hull mesh is not.
    pub(super) fn shown_layers(&self, armor: &LoadedShipArmor) -> ShownLayers {
        let armor_shown = armor.zone_parts.iter().any(|(zone, parts)| {
            parts.iter().any(|part| self.part_visibility.get(&(zone.clone(), part.clone())) != Some(&false))
        });
        let hull_shown = armor.hull_meshes.iter().any(|mesh| self.hull_visibility.get(&mesh.name) == Some(&true));

        ShownLayers { armor: armor_shown, hull: hull_shown }
    }

    /// What the scene handed to this pane has for the option rail to act on. `None` while nothing
    /// has been.
    pub(super) fn scene_features(&self) -> Option<SceneFeatures> {
        self.shown().map(|armor| SceneFeatures { ship: armor.is_ship(), armor: armor.has_armor() })
    }

    /// What this pane's scene was loaded from, which seeds a compare pane's own source.
    pub(super) fn scene_source(&self) -> Option<SceneSource> {
        self.scene_source.clone()
    }

    /// The param index of the ship on screen. A load still in flight has set its source but is
    /// not on screen, so it does not count.
    pub(super) fn loaded_param_index(&self) -> Option<&str> {
        self.current_armor.as_ref()?;
        self.scene_source.as_ref()?.param_index()
    }

    /// The model or ship shown, for the sidebar to highlight. `None` while nothing has been
    /// handed to this pane.
    pub(super) fn open_target(&self) -> Option<OpenTarget> {
        let target = match &self.shown()?.subject {
            SceneSubject::Ship(ship) => OpenTarget::Ship(ship.vehicle_param_index.clone()),
            SceneSubject::Model(model) => match model.from_building {
                Some(building) => OpenTarget::Building(building),
                None => OpenTarget::Model { model: model.path.clone(), from_ship: model.from_ship.clone() },
            },
        };

        Some(target)
    }

    pub(super) fn camera(&self) -> ArcballCamera {
        self.viewport.camera.clone()
    }

    /// Sets the camera without reporting a change, for a pane mirroring another's camera: a
    /// report here would echo back.
    pub(super) fn set_camera(&mut self, camera: ArcballCamera, cx: &mut ViewCx) {
        self.viewport.camera = camera;
        self.viewport.mark_dirty();
        cx.notify();
    }

    /// The settings a pane hands the others while "sync options" is on.
    pub(super) fn synced_settings(&self) -> SyncedSettings {
        SyncedSettings {
            part_visibility: self.part_visibility.clone(),
            plate_visibility: self.plate_visibility.clone(),
            hull_visibility: self.hull_visibility.clone(),
            display_settings: self.display_settings,
            lighting: self.viewport.lighting.clone(),
            camera_rings: self.camera_rings.clone(),
            perspective: self.perspective,
            mesh_seed: self.mesh_seed,
        }
    }

    /// Adopts another pane's settings without reporting a change or recording an undo step.
    pub(super) fn apply_synced(&mut self, settings: &SyncedSettings, cx: &mut ViewCx) {
        self.part_visibility = settings.part_visibility.clone();
        self.mesh_seed = settings.mesh_seed;
        self.plate_visibility = settings.plate_visibility.clone();
        self.hull_visibility = settings.hull_visibility.clone();
        // The source's names are another ship's when the panes differ.
        if let Some(armor) = self.current_armor.clone() {
            self.reseed_mesh(&armor);
        }
        self.display_settings = settings.display_settings;
        self.viewport.lighting = settings.lighting.clone();
        self.camera_rings = settings.camera_rings.clone();
        // Through the toggle rather than by assignment: turning the lock on keeps the free camera
        // to put back, and turning it off puts it back.
        let locked = settings.perspective.enabled;
        self.perspective.camera = settings.perspective.camera;
        self.set_perspective_enabled(locked, cx);
        self.reupload_current_armor(cx);
        self.viewport.mark_dirty();
        cx.notify();
    }

    /// Rebuilds each map `mesh_seed` marks from `armor`'s own names.
    pub(super) fn reseed_mesh(&mut self, armor: &LoadedShipArmor) {
        let saved = AllVisible { hull: self.hull_all_visible, armor: self.armor_all_visible };
        let reseed = OpeningVisibility::new(armor, saved, OpeningLayer::Mesh);
        if self.mesh_seed.armor {
            self.part_visibility = reseed.parts;
        }
        if self.mesh_seed.hull {
            self.hull_visibility = reseed.hull;
        }
    }

    /// Shows `armor`, opening on `layer`. Without a device it is held until there is one.
    ///
    /// Any reload in flight against the scene replaced is dropped, whether or not the device is
    /// there.
    pub(super) fn show_armor(&mut self, armor: Arc<LoadedShipArmor>, layer: OpeningLayer, cx: &mut ViewCx) {
        self.reload_generation += 1;
        self.reload_load.cancel();
        // What the background stage of the scene replaced would still add is of no use now.
        self.rest_load = None;
        if self.scene().is_some() {
            self.upload_armor_now(armor, layer, cx);
            self.pending_armor = None;
        } else {
            self.pending_armor = Some((armor, layer));
            self.upload_hardpoints();
        }
        cx.notify();
    }

    /// Uploads `armor` and keeps it as the scene shown. A new scene starts at the stock hull, the
    /// highest detail and stock camo, with the saved visibility, nothing hovered or undoable, and
    /// the camera framed on it. A no-op without a device.
    pub(super) fn upload_armor_now(&mut self, armor: Arc<LoadedShipArmor>, layer: OpeningLayer, cx: &mut ViewCx) {
        if self.scene().is_none() {
            return;
        }
        let layer = layer.for_scene(&armor);
        self.plate_visibility.clear();
        self.hull_mesh_ids.clear();
        self.expanded_hull_groups.clear();
        self.expanded_camo_groups.clear();
        self.selected_hull = None;
        self.hull_lod = load_ship::DEFAULT_LOD;
        self.selected_modules.clear();
        self.camera_rings.mode = first_camera_mode(&armor);
        self.selected_camo = None;
        self.camo_generation = self.camo_generation.wrapping_add(1);
        self.camo_texture_cache.clear();
        self.active_camo_textures.clear();
        self.active_camo_uvs.clear();
        self.undo_stack.clear();
        self.expanded_zones.clear();
        self.expanded_parts.clear();
        self.hovered = None;
        self.hover_highlight = None;
        self.hovered_ring = None;
        self.hit_pulses.clear();
        self.ring_hovers.clear();
        self.sidebar_highlight = None;
        self.hovered_mesh = None;
        self.mesh_highlight = None;
        self.hull_shown.clear();
        self.hull_textures.clear();
        let saved = AllVisible { hull: self.hull_all_visible, armor: self.armor_all_visible };
        let opening = OpeningVisibility::new(&armor, saved, layer);
        self.part_visibility = opening.parts;
        self.hull_visibility = opening.hull;
        let mesh = layer == OpeningLayer::Mesh;
        self.mesh_seed = MeshSeed { armor: mesh, hull: mesh };
        self.model_bounds = Some(armor.bounds);
        self.scene_problem = None;
        // The arcs were cast at the plates of the ship before; on this one they mark nothing.
        self.arcs.clear();
        self.selected_arc = None;
        let shown = self.shown_layers(&armor);
        let (width, height) = self.viewport_size();
        self.viewport.camera = opening_camera(&armor, shown, width / height.max(1.0));
        self.home_camera = Some(self.viewport.camera.clone());
        // The camera the lock puts back is the one framed on the ship now showing. A ship that
        // names no orbit has nothing to lock onto, and its control is not drawn, so the lock comes
        // off rather than being left on with no way out.
        if self.perspective.enabled {
            if armor.camera_trajectories.is_empty() {
                self.perspective.enabled = false;
                self.camera_before_lock = None;
            } else {
                self.camera_before_lock = Some(self.viewport.camera.clone());
            }
        }
        self.current_armor = Some(armor);
        self.reupload_current_armor(cx);
    }

    /// Outlines the named splash boxes while they are shown, and keeps their names for the
    /// overlay. The outlines are meshes and the names are not, so both are made here to stay in
    /// step.
    pub(super) fn upload_splash_boxes(&mut self, scene: &SceneDevice, armor: &LoadedShipArmor) {
        self.splash_labels.clear();
        let Some(data) = armor.splash_data.as_ref().filter(|_| self.show_splash_boxes) else { return };

        let (vertices, indices) = splash::build_splash_box_wireframes(&data.boxes);
        if !indices.is_empty() {
            self.viewport.add_non_pickable_mesh(scene.device(), &vertices, &indices, LAYER_OVERLAY);
        }
        self.splash_labels = splash::box_labels(&data.boxes);
    }

    /// Marks the hardpoints the sidebar asked for that belong to the scene shown, and keeps their
    /// names for the labels. The names are kept without a device too, so they always say what is
    /// marked.
    pub(super) fn upload_hardpoints(&mut self) {
        self.hovered_hardpoint = None;
        // Before the first layout the view's height is unknown, so no on-screen floor is applied.
        let world_per_pixel = self.last_bounds.map_or(0.0, |bounds| self.viewport.camera.world_per_pixel(bounds.h));
        self.build_hardpoints(world_per_pixel);
    }

    /// [`Self::upload_hardpoints`] with markers held to a least size of `world_per_pixel` per
    /// pixel.
    pub(super) fn build_hardpoints(&mut self, world_per_pixel: f32) {
        if let Some(id) = self.hardpoint_mesh.take() {
            self.viewport.remove_mesh(id);
        }
        let Some(shown) = self.shown().cloned() else {
            self.hardpoint_labels.clear();
            return;
        };

        let markers = HardpointMarkers::for_scene(&shown, &self.hardpoint_targets, world_per_pixel);
        self.hardpoint_labels = markers.labels;
        self.hardpoint_world_per_pixel = world_per_pixel;
        let Some(scene) = self.scene() else { return };
        if !markers.indices.is_empty() {
            self.hardpoint_mesh =
                Some(self.viewport.add_overlay_mesh(scene.device(), &markers.vertices, &markers.indices));
        }
        self.viewport.mark_dirty();
    }

    /// Adopts whose hardpoints are marked, redrawing only the markers.
    pub(super) fn set_hardpoint_targets(&mut self, targets: HashSet<HardpointTarget>, cx: &mut ViewCx) {
        if self.hardpoint_targets == targets {
            return;
        }
        self.hardpoint_targets = targets;
        self.upload_hardpoints();
        cx.notify();
    }

    /// The marked hardpoints, as their labels name them.
    pub(super) fn hardpoint_labels(&self) -> &[HardpointLabel] {
        &self.hardpoint_labels
    }

    /// Whether plates of 0 mm are drawn: when asked for, and always for a model whose plates have
    /// no known thickness at all.
    pub(super) fn show_zero_mm(&self) -> bool {
        match self.shown() {
            Some(armor) => armor.effective_display(self.display_settings).show_zero_mm,
            None => self.display_settings.show_zero_mm,
        }
    }

    /// How far the hull is heeled over, in degrees.
    pub(super) fn model_roll_deg(&self) -> f32 {
        self.viewport.model_roll.to_degrees()
    }

    /// Heels the hull over. Only the model turns: the waterline and the other world-space
    /// overlays stay level, which is what makes the angle readable.
    pub(super) fn set_model_roll_deg(&mut self, degrees: f32, cx: &mut ViewCx) {
        let radians = degrees.to_radians();
        if (self.viewport.model_roll - radians).abs() < f32::EPSILON {
            return;
        }
        self.viewport.model_roll = radians;
        self.viewport.mark_dirty();
        cx.notify();
    }

    /// Whether everything is currently shown, which is what the next ship loaded starts from.
    ///
    /// A part with no entry is visible and a hull mesh with no entry is not, so an empty
    /// `part_visibility` means every part is on while an empty `hull_visibility` means no hull
    /// mesh is.
    pub(super) fn all_visible(&self) -> AllVisible {
        AllVisible {
            hull: if self.mesh_seed.hull {
                self.hull_all_visible
            } else {
                !self.hull_visibility.is_empty() && self.hull_visibility.values().all(|visible| *visible)
            },
            armor: if self.mesh_seed.armor {
                self.armor_all_visible
            } else {
                self.part_visibility.values().all(|visible| *visible)
                    && !self.plate_visibility.values().any(|hidden| *hidden)
            },
        }
    }

    /// How the camera orbits are being drawn.
    pub(super) fn camera_rings(&self) -> CameraRingSettings {
        self.camera_rings.clone()
    }

    /// The camera modes this ship's own GameParams name, in their own order.
    pub(super) fn camera_modes(&self) -> Vec<String> {
        self.current_armor
            .as_ref()
            .map(|armor| armor.camera_trajectories.iter().map(|(name, _)| name.clone()).collect())
            .unwrap_or_default()
    }

    pub(super) fn set_camera_rings(&mut self, settings: CameraRingSettings, cx: &mut ViewCx) {
        if self.camera_rings == settings {
            return;
        }
        self.camera_rings = settings;
        self.reupload_current_armor(cx);
        cx.notify();
    }

    pub(super) fn perspective(&self) -> PerspectiveSettings {
        self.perspective
    }

    /// Locks the camera to the ship's own orbit, or lets it go again.
    ///
    /// The free camera is kept while the lock is on and put back when it comes off, and the lock
    /// starts where the free camera was looking from.
    pub(super) fn set_perspective_enabled(&mut self, on: bool, cx: &mut ViewCx) {
        if on == self.perspective.enabled {
            return;
        }
        if on {
            self.camera_before_lock = Some(self.viewport.camera.clone());
            self.perspective.camera.yaw = self.viewport.camera.azimuth;
            self.perspective.camera.pitch = self.viewport.camera.elevation;
            self.perspective.camera.clamp();
        } else if let Some(camera) = self.camera_before_lock.take() {
            self.viewport.camera = camera;
        }
        self.perspective.enabled = on;
        self.reupload_current_armor(cx);
        cx.notify();
    }

    /// How wide the locked camera sees, in degrees.
    pub(super) fn set_perspective_fov(&mut self, degrees: f32, cx: &mut ViewCx) {
        if (self.perspective.camera.fov_deg - degrees).abs() < f32::EPSILON {
            return;
        }
        self.perspective.camera.fov_deg = degrees;
        self.perspective.camera.clamp();
        self.reupload_current_armor(cx);
        cx.notify();
    }

    /// Whether the locked camera looks along its bearing as the game's does, or through the
    /// ship's centre.
    pub(super) fn set_perspective_look_mode(&mut self, mode: LookMode, cx: &mut ViewCx) {
        if self.perspective.camera.look_mode == mode {
            return;
        }
        self.perspective.camera.look_mode = mode;
        self.reupload_current_armor(cx);
        cx.notify();
    }

    /// The trajectory the locked camera and the drawn orbits both follow: the mode the reader
    /// picked, among the ones this ship names.
    fn current_trajectory(&self) -> Option<&CameraTrajectory> {
        let armor = self.current_armor.as_ref()?;
        let wanted = self.camera_rings.mode.as_ref();
        armor
            .camera_trajectories
            .iter()
            .find(|(name, _)| Some(name) == wanted)
            .or_else(|| armor.camera_trajectories.first())
            .map(|(_, trajectory)| trajectory)
    }

    /// Points the viewport's camera where the lock says, and marks what it is aimed at.
    pub(super) fn apply_perspective(&mut self, scene: &SceneDevice, armor: &LoadedShipArmor) {
        self.aim_perspective(Some(scene), armor.bounds);
    }

    /// Points the camera where the lock says, for a change that moved the lock rather than the
    /// model. Cheap enough for a drag: the armor is not touched, only the camera and the marker
    /// saying where it is aimed.
    pub(super) fn refresh_perspective(&mut self, cx: &mut ViewCx) {
        let scene = self.scene();
        let Some(bounds) = self.model_bounds else { return };
        self.aim_perspective(scene.as_deref(), bounds);
        self.viewport.mark_dirty();
        cx.notify();
    }

    /// Points the viewport's camera where the lock says, and marks what it is aimed at when there
    /// is a device to build the marker with.
    ///
    /// A no-op while the lock is off, and while the ship names no orbit to lock onto.
    fn aim_perspective(&mut self, scene: Option<&SceneDevice>, bounds: (Vec3, Vec3)) {
        for id in self.aim_mesh_ids.drain(..) {
            self.viewport.remove_mesh(id);
        }
        if !self.perspective.enabled {
            return;
        }
        let Some(trajectory) = self.current_trajectory().cloned() else { return };

        // The orbits sit at the height the trajectory states, as the drawn rings do: the armor
        // carries no waterline offset.
        let waterline_dy = 0.0;
        let (fov, height) = (self.camera_rings.fov, self.camera_rings.height);
        self.perspective.camera.clamp_pitch_to_far_side(&trajectory, fov, height, waterline_dy);
        let (eye_model, look_model) = self.perspective.camera.eye_and_look_dir(&trajectory, fov, height, waterline_dy);
        let eye = self.viewport.pos_to_world_space(eye_model);
        let look = self.viewport.pos_to_world_space(look_model);
        let target = water_aim_point(eye, look, AIM_MARKER_REACH);

        let (min, max) = bounds;
        let diagonal = (max - min).norm();
        let camera = &mut self.viewport.camera;
        camera.set_eye_and_target(eye, target);
        camera.fov = self.perspective.camera.fov_deg.to_radians();
        // Near enough to stand on the ship without the hull clipping away, far enough to keep the
        // water the eye is aimed at in the frustum.
        camera.near = 0.05;
        camera.far = (diagonal * 8.0).max(10.0);

        let Some(scene) = scene else { return };
        let radius = (diagonal * AIM_MARKER_FRACTION).clamp(AIM_MARKER_MIN, AIM_MARKER_MAX);
        let (vertices, indices) = camera_rings::build_water_marker(target, radius, AIM_MARKER_COLOR);
        if !indices.is_empty() {
            let id = self.viewport.add_world_space_mesh(scene.device(), &vertices, &indices, LAYER_OVERLAY);
            self.aim_mesh_ids.push(id);
        }
    }

    /// Whether the free camera is the reader's to move. While the lock is on the camera belongs
    /// to the ship's orbit, and anything that moved it another way would be undone by the next
    /// aim.
    pub(super) fn camera_is_free(&self) -> bool {
        !self.perspective.enabled
    }

    /// Turns the locked camera by a drag, in pixels. Says whether it moved.
    pub(super) fn steer_perspective(&mut self, dx: f32, dy: f32) -> bool {
        if !self.perspective.enabled || (dx == 0.0 && dy == 0.0) {
            return false;
        }
        self.perspective.camera.yaw -= dx * PERSPECTIVE_YAW_PER_PX;
        self.perspective.camera.pitch += dy * PERSPECTIVE_PITCH_PER_PX;
        self.perspective.camera.clamp();
        true
    }

    /// Moves the locked camera between the inner and outer orbit. Says whether it moved.
    pub(super) fn zoom_perspective(&mut self, notches: f32) -> bool {
        if !self.perspective.enabled || notches == 0.0 {
            return false;
        }
        self.perspective.camera.zoom -= notches * PERSPECTIVE_ZOOM_PER_NOTCH;
        self.perspective.camera.clamp();
        true
    }

    pub(super) fn has_armor(&self) -> bool {
        self.current_armor.is_some()
    }

    /// The last plate the pointer was over: its zone and its thickness.
    pub(super) fn last_plate(&self) -> Option<(String, f32)> {
        self.last_plate.clone()
    }

    pub(super) fn shown_ship_name(&self) -> Option<String> {
        self.scene_source.as_ref().map(|source| source.display_name().to_string())
    }

    /// Names the subject being loaded into this viewport, which the pane's load drives.
    pub(super) fn set_ship_loading(&mut self, loading: Option<String>, cx: &mut ViewCx) {
        if self.ship_loading == loading {
            return;
        }
        self.ship_loading = loading;
        cx.notify();
    }

    /// Starts a load into this pane, naming it while it runs, and cancels the first stage of the
    /// one before. A result lands only while its generation is still the latest one this pane
    /// started, so a newer selection here supersedes it and one in another pane does not.
    pub(super) fn begin_load(&mut self, name: String, cx: &mut ViewCx) -> OpeningLoad {
        self.load_generation = LoadGeneration(self.load_generation.0 + 1);
        let cancel = self.opening_load.start();
        self.scene_problem = None;
        self.set_ship_loading(Some(name), cx);

        OpeningLoad { generation: self.load_generation, cancel }
    }

    pub(super) fn is_current_load(&self, generation: LoadGeneration) -> bool {
        self.load_generation == generation
    }

    /// Cancels the load in flight, so it stops and its result is dropped. Whether one was in
    /// flight.
    pub(super) fn cancel_load(&mut self, cx: &mut ViewCx) -> bool {
        if self.ship_loading.is_none() {
            return false;
        }

        self.load_generation = LoadGeneration(self.load_generation.0 + 1);
        self.opening_load.cancel();
        self.set_ship_loading(None, cx);

        true
    }

    /// Runs the background stage of `scene`, which this viewport has just been given to show.
    /// Each job stores its piece in the scene as it lands, and every viewport showing the scene
    /// then takes it up. Cancelled once no viewport shows the scene.
    pub(super) fn start_scene_rest(&mut self, scene: Arc<LoadedShipArmor>, rest: Vec<RestJob>, cx: &mut ViewCx) {
        let run = SharedLoad::start();
        let cancel = run.cancel_flag().clone();
        self.rest_load = Some(run);
        cx.defer(move |_state| {
            for job in rest {
                let piece = job.piece();
                let job_scene = Arc::clone(&scene);
                let job_cancel = cancel.clone();
                let scene = Arc::clone(&scene);
                spawn_local(async move {
                    match blocking(move || job.run(&job_scene, &job_cancel)).await {
                        Ok(()) => {}
                        Err(RestJobError::Cancelled(_)) => return,
                        Err(e) => {
                            tracing::warn!("armor viewer: {} lacks its {piece:?}: {e}", scene.display_name);
                            return;
                        }
                    }
                    // Every viewport rather than this one, which may have closed while another
                    // still shows the scene.
                    for viewport in ArmorViewportState::live() {
                        viewport.update(|this, cx| this.take_up_rest(&scene, piece, cx));
                    }
                });
            }
        });
    }

    /// The background stage of the scene shown, for a viewport that is given the same scene to
    /// hold too.
    pub(super) fn scene_rest(&self) -> Option<Arc<SharedLoad>> {
        self.rest_load.clone()
    }

    /// Holds `run`, the background stage of the scene just shown here, which another viewport
    /// started.
    pub(super) fn share_scene_rest(&mut self, run: Arc<SharedLoad>) {
        self.rest_load = Some(run);
    }

    /// Takes up `piece` of `scene`, when it is the scene shown.
    fn take_up_rest(&mut self, scene: &Arc<LoadedShipArmor>, piece: RestPiece, cx: &mut ViewCx) {
        // A scene still waiting for the device reads every piece landed so far once it is
        // uploaded.
        if !self.current_armor.as_ref().is_some_and(|shown| Arc::ptr_eq(shown, scene)) {
            return;
        }

        match piece {
            // A pointer resting on the hull while the index was built hovers it now, as a move
            // there would.
            RestPiece::Pick => self.hover_again(),
            // Read by the next draw of the camo picker.
            RestPiece::Camo => {}
            RestPiece::SurfaceMaps => self.surface_maps_arrived(),
        }
        cx.notify();
    }

    /// Picks the hover again where the pointer last moved over the view.
    fn hover_again(&mut self) {
        let Some(at) = self.last_pointer else { return };
        let over_view = self.last_bounds.is_some_and(|bounds| bounds.contains(at));
        if over_view && self.drag.is_none() && self.camera_is_free() {
            self.update_plate_hover(at);
        }
    }

    /// Draws the hull again with the surface maps that have landed. A material's cache key names
    /// its maps, so the uploaded albedos are reused.
    fn surface_maps_arrived(&mut self) {
        self.reupload_hull();
    }

    /// Shows `problem` in place of the scene. The scene stays loaded, so the selectors that led
    /// here can pick a level of detail or hull that draws.
    pub(super) fn show_problem(&mut self, problem: SceneProblem, cx: &mut ViewCx) {
        self.scene_problem = Some(problem);
        cx.notify();
    }

    /// Drops the scene and shows `problem` in its place. The failed load was for another subject,
    /// which the old scene must not pass for: the sidebar and option rail would go on describing
    /// it.
    pub(super) fn fail_load(&mut self, problem: SceneProblem, cx: &mut ViewCx) {
        self.clear_scene();
        self.show_problem(problem, cx);
    }

    pub(super) fn scene_problem(&self) -> Option<&SceneProblem> {
        self.scene_problem.as_ref()
    }

    /// Returns the pane to showing no scene, as it starts.
    fn clear_scene(&mut self) {
        // A reload in flight was for the scene going away.
        self.reload_generation += 1;
        self.reload_load.cancel();
        self.rest_load = None;
        self.current_armor = None;
        self.pending_armor = None;
        self.scene_source = None;
        self.model_bounds = None;
        self.home_camera = None;
        self.hovered = None;
        self.hover_highlight = None;
        self.hovered_ring = None;
        self.ring_hovers.clear();
        self.sidebar_highlight = None;
        self.hovered_mesh = None;
        self.mesh_highlight = None;
        self.hull_shown.clear();
        self.hit_pulses.clear();
        self.arcs.clear();
        self.selected_arc = None;
        self.hits.clear();
        self.hit_health = None;
        self.sim_verdicts.clear();
        self.sim_unchecked = 0;
        self.last_plate = None;
        self.splash_result = None;
        self.splash_labels.clear();
        self.gap_count = 0;

        self.viewport.clear();
        self.mesh_triangle_info.clear();
        self.hull_mesh_ids.clear();
        self.aim_mesh_ids.clear();
        self.hardpoint_mesh = None;
        self.length_marks = None;
        self.upload_hardpoints();
    }

    /// The names of the splash boxes and where each is written, while the boxes are outlined.
    pub(super) fn splash_box_labels(&self) -> &[splash::SplashBoxLabel] {
        &self.splash_labels
    }
}

impl ArmorViewportState {
    pub fn apply_armor_defaults(self, defaults: Option<&ArmorViewerDefaultsRow>) {
        self.update(|this, cx| this.apply_armor_defaults(defaults, cx));
    }

    /// The assets the scene was loaded from.
    pub fn reload_assets(self) -> Option<Arc<ShipAssets>> {
        self.read(|this| this.reload_assets()).flatten()
    }

    pub fn shown_species(self) -> Option<Species> {
        self.read(|this| this.shown_species()).flatten()
    }

    /// `material` named as the game names it on the ship shown.
    pub fn material_label(self, material: ArmorMaterial) -> String {
        self.read(|this| this.material_label(material)).unwrap_or_else(|| material.english_label())
    }

    pub fn set_scene_source(self, source: SceneSource) {
        self.update(|this, _cx| this.set_scene_source(source));
    }

    /// The scene last handed to this viewport.
    pub fn shown(self) -> Option<Arc<LoadedShipArmor>> {
        self.read(|this| this.shown().cloned()).flatten()
    }

    pub fn shown_untracked(self) -> Option<Arc<LoadedShipArmor>> {
        self.read_untracked(|this| this.shown().cloned()).flatten()
    }

    pub fn scene_features(self) -> Option<SceneFeatures> {
        self.read(|this| this.scene_features()).flatten()
    }

    pub fn scene_source(self) -> Option<SceneSource> {
        self.read(|this| this.scene_source()).flatten()
    }

    pub fn loaded_param_index(self) -> Option<String> {
        self.read(|this| this.loaded_param_index().map(str::to_owned)).flatten()
    }

    pub fn open_target(self) -> Option<OpenTarget> {
        self.read(|this| this.open_target()).flatten()
    }

    pub fn camera(self) -> ArcballCamera {
        self.read(|this| this.camera()).unwrap_or_default()
    }

    /// Sets the camera without reporting a change.
    pub fn set_camera(self, camera: ArcballCamera) {
        self.update(|this, cx| this.set_camera(camera, cx));
    }

    pub fn synced_settings(self) -> Option<SyncedSettings> {
        self.read(|this| this.synced_settings())
    }

    /// Adopts another pane's settings without reporting a change.
    pub fn apply_synced(self, settings: &SyncedSettings) {
        self.update(|this, cx| this.apply_synced(settings, cx));
    }

    pub fn show_armor(self, armor: Arc<LoadedShipArmor>, layer: OpeningLayer) {
        self.update(|this, cx| this.show_armor(armor, layer, cx));
    }

    pub fn set_hardpoint_targets(self, targets: HashSet<HardpointTarget>) {
        self.update(|this, cx| this.set_hardpoint_targets(targets, cx));
    }

    pub fn hardpoint_labels(self) -> Vec<HardpointLabel> {
        self.read(|this| this.hardpoint_labels().to_vec()).unwrap_or_default()
    }

    pub fn show_zero_mm(self) -> bool {
        self.read(|this| this.show_zero_mm()).unwrap_or_default()
    }

    pub fn model_roll_deg(self) -> f32 {
        self.read(|this| this.model_roll_deg()).unwrap_or_default()
    }

    pub fn set_model_roll_deg(self, degrees: f32) {
        self.update(|this, cx| this.set_model_roll_deg(degrees, cx));
    }

    /// Whether everything is shown. Everything, as a new viewport starts, once the state is gone.
    pub fn all_visible(self) -> AllVisible {
        self.read(|this| this.all_visible()).unwrap_or(AllVisible { hull: false, armor: true })
    }

    pub fn camera_rings(self) -> CameraRingSettings {
        self.read(|this| this.camera_rings()).unwrap_or_default()
    }

    pub fn camera_modes(self) -> Vec<String> {
        self.read(|this| this.camera_modes()).unwrap_or_default()
    }

    pub fn set_camera_rings(self, settings: CameraRingSettings) {
        self.update(|this, cx| this.set_camera_rings(settings, cx));
    }

    pub fn perspective(self) -> PerspectiveSettings {
        self.read(|this| this.perspective()).unwrap_or_default()
    }

    pub fn set_perspective_enabled(self, on: bool) {
        self.update(|this, cx| this.set_perspective_enabled(on, cx));
    }

    pub fn set_perspective_fov(self, degrees: f32) {
        self.update(|this, cx| this.set_perspective_fov(degrees, cx));
    }

    pub fn set_perspective_look_mode(self, mode: LookMode) {
        self.update(|this, cx| this.set_perspective_look_mode(mode, cx));
    }

    pub fn has_armor(self) -> bool {
        self.read(|this| this.has_armor()).unwrap_or_default()
    }

    pub fn last_plate(self) -> Option<(String, f32)> {
        self.read(|this| this.last_plate()).flatten()
    }

    pub fn shown_ship_name(self) -> Option<String> {
        self.read(|this| this.shown_ship_name()).flatten()
    }

    pub fn set_ship_loading(self, loading: Option<String>) {
        self.update(|this, cx| this.set_ship_loading(loading, cx));
    }

    /// Starts a load into this viewport. Once the state is gone the load comes back already
    /// cancelled, so it stops at its first checkpoint.
    pub fn begin_load(self, name: String) -> OpeningLoad {
        self.update(|this, cx| this.begin_load(name, cx)).unwrap_or_else(|| {
            let cancel = LoadCancel::new();
            cancel.cancel();
            OpeningLoad { generation: LoadGeneration::default(), cancel }
        })
    }

    pub fn is_current_load(self, generation: LoadGeneration) -> bool {
        self.read_untracked(|this| this.is_current_load(generation)).unwrap_or(false)
    }

    pub fn cancel_load(self) -> bool {
        self.update(|this, cx| this.cancel_load(cx)).unwrap_or(false)
    }

    pub fn start_scene_rest(self, scene: Arc<LoadedShipArmor>, rest: Vec<RestJob>) {
        self.update(|this, cx| this.start_scene_rest(scene, rest, cx));
    }

    pub fn scene_rest(self) -> Option<Arc<SharedLoad>> {
        self.read_untracked(|this| this.scene_rest()).flatten()
    }

    pub fn share_scene_rest(self, run: Arc<SharedLoad>) {
        self.update(|this, _cx| this.share_scene_rest(run));
    }

    pub fn show_problem(self, problem: SceneProblem) {
        self.update(|this, cx| this.show_problem(problem, cx));
    }

    pub fn fail_load(self, problem: SceneProblem) {
        self.update(|this, cx| this.fail_load(problem, cx));
    }

    pub fn scene_problem(self) -> Option<SceneProblem> {
        self.read(|this| this.scene_problem().cloned()).flatten()
    }

    pub fn splash_box_labels(self) -> Vec<splash::SplashBoxLabel> {
        self.read(|this| this.splash_box_labels().to_vec()).unwrap_or_default()
    }
}

#[cfg(test)]
mod all_visible_tests {
    use std::collections::HashMap;

    use wows_toolkit_armor::load_ship::PlateKey;

    use super::super::model::ViewportModel;
    use super::super::types::AllVisible;

    /// What a viewport holding these maps, untouched by the mesh seed, reports as shown in full.
    fn all_visible(
        hull: HashMap<String, bool>,
        parts: HashMap<(String, String), bool>,
        plates: HashMap<PlateKey, bool>,
    ) -> AllVisible {
        let mut model = ViewportModel::new();
        model.hull_visibility = hull;
        model.part_visibility = parts;
        model.plate_visibility = plates;
        model.all_visible()
    }

    fn hull(entries: &[(&str, bool)]) -> HashMap<String, bool> {
        entries.iter().map(|(name, visible)| ((*name).to_string(), *visible)).collect()
    }

    #[test]
    fn nothing_hidden_reads_as_everything_shown() {
        let shown = all_visible(hull(&[("Hull_A", true)]), HashMap::new(), HashMap::new());
        assert_eq!(shown, AllVisible { hull: true, armor: true });
    }

    #[test]
    fn a_hull_mesh_with_no_entry_is_not_shown() {
        // Absent means hidden on the hull side, so an empty map is not everything.
        let shown = all_visible(HashMap::new(), HashMap::new(), HashMap::new());
        assert!(!shown.hull);
        assert!(shown.armor, "a part with no entry is visible, so an empty map is everything");
    }

    #[test]
    fn one_hidden_part_or_plate_is_enough_to_say_not_everything() {
        let parts = HashMap::from([(("Zone".to_string(), "Part".to_string()), false)]);
        assert!(!all_visible(HashMap::new(), parts, HashMap::new()).armor);

        let plates: HashMap<PlateKey, bool> = HashMap::from([(
            PlateKey { zone: "Zone".to_string(), material_name: "Part".to_string(), thickness_tenths: 0 },
            true,
        )]);
        assert!(!all_visible(HashMap::new(), HashMap::new(), plates).armor);
    }
}

#[cfg(test)]
mod opening_visibility_tests {
    use std::collections::HashMap;

    use wows_toolkit_armor::load_model::fixtures::armored_model_scene;
    use wows_toolkit_armor::load_model::fixtures::model_scene;
    use wows_toolkit_armor::load_ship::ArmorThickness;
    use wows_toolkit_armor::load_ship::SceneSubject;
    use wows_toolkit_armor::load_ship::ThicknessSource;
    use wows_toolkit_armor::model_refs::VehicleParamIndex;
    use wowsunpack::export::hardpoint::HardpointName;
    use wowsunpack::export::model_catalog::ModelPath;

    use super::super::types::AllVisible;
    use super::super::types::OpeningLayer;
    use super::OpeningVisibility;
    use super::ShownLayers;
    use super::opening_camera;

    fn zone_parts() -> Vec<(String, Vec<String>)> {
        vec![("Citadel".to_string(), vec!["Belt".to_string(), "Deck".to_string()])]
    }

    fn hull_groups() -> Vec<(String, Vec<String>)> {
        vec![("Hull".to_string(), vec!["Hull_A".to_string(), "Hull_B".to_string()])]
    }

    const SAVED: AllVisible = AllVisible { hull: false, armor: true };

    #[test]
    fn the_armor_layer_follows_the_saved_defaults() {
        let opening = OpeningVisibility::from_groups(&zone_parts(), &hull_groups(), SAVED, OpeningLayer::Armor);
        assert_eq!(opening, OpeningVisibility::default());
    }

    fn turret() -> ModelPath {
        ModelPath::parse("content/gameplay/usa/gun/main/AGM034/AGM034.model").expect("a model path")
    }

    #[test]
    fn an_armored_model_opens_on_the_chosen_layer() {
        let mut scene = armored_model_scene(turret());
        if let SceneSubject::Model(model) = &mut scene.subject {
            model.thickness = ArmorThickness::Ship(ThicknessSource {
                vehicle_param_index: VehicleParamIndex::new("PASC001".to_string()),
                display_name: "Iowa".to_string(),
                hp: HardpointName::new("HP_AGM_1"),
            });
        }

        assert_eq!(OpeningLayer::Armor.for_scene(&scene), OpeningLayer::Armor);
        assert_eq!(OpeningLayer::Mesh.for_scene(&scene), OpeningLayer::Mesh);
    }

    #[test]
    fn a_model_with_armor_of_unknown_thickness_opens_on_its_meshes() {
        let scene = armored_model_scene(turret());

        assert_eq!(OpeningLayer::Armor.for_scene(&scene), OpeningLayer::Mesh);
    }

    #[test]
    fn a_model_without_armor_opens_on_its_meshes_and_shows_them() {
        let scene = model_scene(turret(), Vec::new());
        let layer = OpeningLayer::Armor.for_scene(&scene);

        assert_eq!(layer, OpeningLayer::Mesh);
        let opening = OpeningVisibility::new(&scene, SAVED, layer);
        assert_eq!(opening.hull.get("Turret"), Some(&true), "the model's mesh is shown");
    }

    #[test]
    fn the_mesh_layer_shows_the_hull_and_hides_the_armor() {
        let opening = OpeningVisibility::from_groups(&zone_parts(), &hull_groups(), SAVED, OpeningLayer::Mesh);
        let parts: HashMap<_, _> = [
            (("Citadel".to_string(), "Belt".to_string()), false),
            (("Citadel".to_string(), "Deck".to_string()), false),
        ]
        .into_iter()
        .collect();
        let hull: HashMap<_, _> = [("Hull_A".to_string(), true), ("Hull_B".to_string(), true)].into_iter().collect();
        assert_eq!(opening, OpeningVisibility { parts, hull });
    }

    /// A model's camera stands far enough off that the whole of what is drawn fits the view, and
    /// a hidden layer reaching further is ignored.
    #[test]
    fn a_model_is_framed_on_the_layers_drawn() {
        let mut scene = armored_model_scene(turret());
        for position in &mut scene.meshes[0].positions {
            *position = position.map(|axis| axis * 100.0);
        }
        let shown = ShownLayers { armor: false, hull: true };

        let camera = opening_camera(&scene, shown, 0.5);

        let target = [camera.target.x, camera.target.y, camera.target.z];
        assert_eq!(target, [2.0, 1.0, 3.0], "centred on the hull mesh");
        // Half the diagonal of the 4 x 2 x 6 box the mesh spans.
        let radius = 56.0_f32.sqrt() * 0.5;
        let half_width = ((camera.fov * 0.5).tan() * 0.5).atan();
        assert!(camera.distance * half_width.sin() >= radius, "{} is too close", camera.distance);
        assert!(camera.distance * half_width.sin() <= radius * 1.5, "{} is too far", camera.distance);
    }
}

#[cfg(test)]
mod real_model_tests {
    use std::collections::HashSet;
    use std::sync::Arc;

    use wows_toolkit_armor::assets::ModelAssets;
    use wows_toolkit_armor::hardpoint_overlay::HardpointMarkers;
    use wows_toolkit_armor::load_model::ModelLoadRequest;
    use wows_toolkit_armor::load_model::load_model_scene;
    use wows_toolkit_armor::model_refs::HardpointTarget;
    use wows_toolkit_armor::scene_load::LoadCancel;
    use wows_toolkit_armor::viewport::types::Vec3;
    use wows_toolkit_armor::viewport::types::ViewRect;
    use wowsunpack::export::armor_ray::transform_point;
    use wowsunpack::export::building_armor::BuildingArmorIndex;
    use wowsunpack::export::mount_armor::MountArmorIndex;
    use wowsunpack::export::ship::ShipAssets;
    use wowsunpack::game_data::AssetsBin;
    use wowsunpack::game_data::build_game_vfs_for_build;
    use wowsunpack::game_data::list_available_builds;
    use wowsunpack::game_params::provider::GameMetadataProvider;

    use super::ShownLayers;
    use super::opening_camera;

    /// Opens catalogued models from a real install, as the viewer opens them, and checks every
    /// vertex drawn is in view from several angles and each hardpoint marker is small beside the
    /// model. Skipped unless `WT_WOWS_DIR` names an install.
    #[test]
    fn real_models_open_framed_whole_with_small_markers() {
        let Ok(wows_dir) = std::env::var("WT_WOWS_DIR") else {
            eprintln!("skipped: WT_WOWS_DIR names no game install");
            return;
        };
        let wows_dir = std::path::PathBuf::from(wows_dir);
        let builds = list_available_builds(&wows_dir).expect("installed builds");
        let build = *builds.last().expect("at least one build");
        let vfs = build_game_vfs_for_build(&wows_dir, build, AssetsBin::Overlay).expect("the game VFS");
        let metadata = Arc::new(GameMetadataProvider::from_vfs(&vfs).expect("GameParams"));
        let assets = ShipAssets::from_vfs_with_metadata(&vfs, Arc::clone(&metadata)).expect("ship assets");
        let library = assets.model_library().expect("the model library");
        let mount_armor = MountArmorIndex::build(&metadata, library.catalog());
        // This test opens no building.
        let buildings = BuildingArmorIndex::default();
        let models = ModelAssets { library: Arc::new(library), mount_armor, buildings };
        let entries = models.library.catalog().entries();
        let turrets = entries.iter().filter(|entry| entry.name() == "AGM034_16in50_Mk7");
        let directors = entries.iter().filter(|entry| entry.name().contains("Director")).take(3);
        let paths: Vec<_> = turrets.chain(directors).map(|entry| entry.path().clone()).collect();
        assert_eq!(paths.len(), 4, "the turret and three directors are catalogued");

        let mut marked = 0;
        for path in paths {
            let cancel = LoadCancel::new();
            let scene = load_model_scene(&models, &metadata, &ModelLoadRequest::new(path.clone(), None), &cancel)
                .expect("the model loads")
                .scene;
            let shown = ShownLayers { armor: true, hull: true };
            let rect = ViewRect::new(0.0, 0.0, 1200.0, 700.0);
            let mut camera = opening_camera(&scene, shown, 1200.0 / 700.0);
            let plates =
                scene.meshes.iter().flat_map(|mesh| mesh.positions.iter().map(|p| (mesh.transform.as_ref(), *p)));
            let hull =
                scene.hull_meshes.iter().flat_map(|mesh| mesh.positions.iter().map(|p| (mesh.transform.as_ref(), *p)));
            let points: Vec<Vec3> = plates
                .chain(hull)
                .map(|(transform, p)| Vec3::from(transform.map_or(p, |t| transform_point(t, p))))
                .collect();
            for (azimuth, elevation) in [(camera.azimuth, camera.elevation), (0.0, 0.0), (2.0, -0.6), (4.0, 1.2)] {
                camera.azimuth = azimuth;
                camera.elevation = elevation;
                for point in &points {
                    let at = camera.project_to_screen(*point, rect).expect("in front of the camera");
                    assert!(rect.contains(at), "{path}: {point:?} at {at:?} is out of view");
                }
            }

            let (min, max) = scene.bounds;
            let radius = (max - min).norm() * 0.5;
            let targets = HashSet::from([HardpointTarget::Model(path.clone())]);
            let world_per_pixel = camera.world_per_pixel(rect.height());
            let markers = HardpointMarkers::for_scene(&scene, &targets, world_per_pixel);
            let hardpoints = scene.model().map_or(0, |model| model.hardpoints.len());
            assert_eq!(markers.labels.len(), hardpoints, "{path}: every hardpoint is marked");
            marked += hardpoints;
            let reach = markers
                .labels
                .iter()
                .enumerate()
                .flat_map(|(index, label)| {
                    let first = index * (6 + 3 * 8);
                    markers.vertices[first..first + 6 + 3 * 8]
                        .iter()
                        .map(move |vertex| (Vec3::from(vertex.position) - Vec3::from(label.position)).norm())
                })
                .fold(0.0_f32, f32::max);
            eprintln!(
                "{path}: radius {radius:.2}, camera {:.2} off, {} markers reaching {reach:.3}",
                camera.distance,
                markers.labels.len()
            );
            assert!(reach <= radius * 0.1, "{path}: a marker reaches {reach} on a model {radius} across");
        }
        assert!(marked > 0, "some marker was measured");
    }
}
