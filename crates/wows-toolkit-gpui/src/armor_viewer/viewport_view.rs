//! Armor viewport view: presents one [`ArmorScene`] as an `img()` of its
//! offscreen render, forwards gpui mouse, wheel and key input to it, and
//! draws the navigation gizmo, the plate and orbit tooltips, the shot log,
//! the context menu and the export confirmation around it.
//!
//! Rendering is on demand: the scene re-renders only while it is dirty, once
//! per notified frame. The wgpu device is shared by every pane
//! (`ArmorViewerPane`'s `SharedGpu`) and handed down through
//! [`ViewportView::set_gpu`]; until then the view shows a status message.

use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use gpui_kit::component::ActiveTheme;
use gpui_kit::component::button::Button;
use gpui_kit::component::h_flex;
use gpui_kit::component::menu::ContextMenuExt;
use gpui_kit::component::menu::PopupMenuItem;
use gpui_kit::component::slider::SliderEvent;
use gpui_kit::component::slider::SliderState;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::v_flex;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use rust_i18n::t;
use wows_replay_insights::timeline::PreExtractedHit;
use wows_replays::types::GameParamId;
use wows_replays::types::ShotId;
use wows_toolkit_armor::load_ship::LoadedShipArmor;
use wows_toolkit_armor::load_ship::PlateKey;
use wows_toolkit_armor::scene;
use wows_toolkit_armor::scene::ArmorScene;
use wows_toolkit_armor::scene::CameraRingSettings;
use wows_toolkit_armor::scene::GizmoView;
use wows_toolkit_armor::scene::Gpu;
use wows_toolkit_armor::scene::PaneCommand;
use wows_toolkit_armor::upload;
use wows_toolkit_armor::viewport::device::GpuContext;
use wows_toolkit_armor::viewport::gizmo;
use wows_toolkit_armor::viewport::renderer::GpuPipeline;
use wows_toolkit_armor::viewport::types::LightingSettings;
use wows_toolkit_armor::viewport::types::Vec2;
use wows_toolkit_armor::viewport::types::ViewRect;
use wows_toolkit_config::queries::ArmorViewerDefaultsRow;
use wows_toolkit_viewmodel::armor::camera_perspective::CameraPerspective;
use wowsunpack::export::camo_textures::CamoSchemeId;
use wowsunpack::game_params::keys::ComponentType;
use wowsunpack::game_params::ttx::armor_material::ArmorMaterial;
use wowsunpack::game_params::types::Km;
use wowsunpack::game_params::types::ShellInfo;
use wowsunpack::game_params::types::Species;

use super::analysis::SimAgreement;
use super::render_image::readback_to_render_image;
use crate::armor_viewer::assets::ArmorAssetsBundle;
use crate::armor_viewer::load_ship;
use crate::armor_viewer::load_ship::spawn_reload_ship_armor;
use crate::armor_viewer::pane::ArmorViewerPane;
use crate::armor_viewer::picking_ui;
use crate::armor_viewer::popover;

/// How often the scene is ticked while it has held keys or a snap
/// animation to advance.
const TICK_HZ: f32 = 60.0;

/// Offset (in each axis) from the cursor to the thickness tooltip's anchor
/// position, so the tooltip doesn't sit directly under the pointer.
const TOOLTIP_CURSOR_OFFSET: Pixels = px(16.);

/// Cross-pane events: `pane.rs` subscribes to every pane's `ViewportView`
/// and, when `mirror_cameras`/`sync_options` is on, broadcasts the source
/// pane's camera/settings to every other pane. Fired from the scene's
/// reported changes, which the silent counterparts
/// [`ViewportView::set_camera`]/[`ViewportView::apply_synced`] never report,
/// so the broadcast does not echo back into a loop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ViewportChange {
    Camera,
    Settings,
    TrajectoryMode,
}

/// The display-settings popover's numeric sliders (`popover.rs`). A
/// persistent `Entity<SliderState>` per slider so its thumb position
/// survives the popover closing and reopening; rebuilt wholesale whenever the
/// seed changes outside a drag (`apply_armor_defaults`), since
/// `SliderState::set_value` needs a `&mut Window` that isn't available there.
pub(crate) struct DisplaySettingsSliders {
    pub(crate) waterline_opacity: Entity<SliderState>,
    pub(crate) armor_opacity: Entity<SliderState>,
    /// How solid the impact markers are drawn.
    pub(crate) marker_opacity: Entity<SliderState>,
    /// How wide the locked camera sees, in degrees.
    pub(crate) perspective_fov: Entity<SliderState>,
    /// How far the hull is heeled over, in degrees. A property of the model
    /// on screen rather than of the armor, so it is not written back with the
    /// display defaults.
    pub(crate) model_roll_deg: Entity<SliderState>,
    /// Where between the field-of-view extremes the drawn camera orbits sit.
    pub(crate) camera_fov: Entity<SliderState>,
    /// How far those orbits are raised.
    pub(crate) camera_height: Entity<SliderState>,
    /// The range a cast shell is fired from, in kilometres, which sets how
    /// steeply it comes down onto the armor.
    pub(crate) cast_range: Entity<SliderState>,
}

/// The two colours the lighting is made of, as the pickers that set them.
///
/// Built with the view rather than with the popover: a picker keeps its own
/// sliders and its open state, and one rebuilt per frame would close itself as
/// the reader dragged it.
pub(crate) struct LightingColors {
    pub(crate) flat: Entity<gpui_kit::component::color_picker::ColorPickerState>,
    pub(crate) key: Entity<gpui_kit::component::color_picker::ColorPickerState>,
}

/// The display-settings popover's lighting sliders, same persistent-entity
/// rationale as [`DisplaySettingsSliders`]. Rebuilt wholesale by a preset
/// button (`ViewportView::set_lighting_preset`), which moves every slider at
/// once.
pub(crate) struct LightingSliders {
    pub(crate) flat_intensity: Entity<SliderState>,
    pub(crate) key_intensity: Entity<SliderState>,
    pub(crate) azimuth_deg: Entity<SliderState>,
    pub(crate) elevation_deg: Entity<SliderState>,
    pub(crate) rim_strength: Entity<SliderState>,
    pub(crate) specular_strength: Entity<SliderState>,
    pub(crate) shininess: Entity<SliderState>,
}

/// What a background reload needs: the shared ship-export bundle, this ship's
/// param index, and its display name. Set via
/// [`ViewportView::set_reload_source`] whenever the pane loads a ship; a
/// reload itself never replaces it.
struct ReloadSource {
    bundle: Arc<ArmorAssetsBundle>,
    param_index: String,
    display_name: String,
}

pub struct ViewportView {
    focus_handle: FocusHandle,
    /// The ship, its settings, and what input does to them.
    scene: ArmorScene,
    /// Why the pane's shared device failed to start, shown in place of the
    /// viewport. `None` while it is starting or once it is ready.
    gpu_failure: Option<String>,
    /// The container's window-space bounds as of the last paint. `None` until
    /// the first layout pass; the redraw waits on it.
    last_bounds: Option<Bounds<Pixels>>,
    image: Option<Arc<RenderImage>>,
    /// Whether the scene ticker is running, so input that wants ticks does
    /// not stack a second one.
    ticking: bool,
    /// The ship being loaded into this viewport, if one is. Set by the pane
    /// (`set_ship_loading`), which owns the load; the viewport shows it in
    /// place of whatever it was drawing, matching the egui pane
    /// (`ui/tab.rs:2081`).
    ship_loading: Option<SharedString>,
    /// Which hull-part-group headers are expanded in the hull popover.
    /// Collapsed by default and whenever a new ship loads.
    pub(crate) expanded_hull_groups: HashSet<String>,
    /// Which camo-origin group headers are expanded in the camo picker.
    pub(crate) expanded_camo_groups: HashSet<String>,
    /// Which zone headers are expanded in the visibility popover's tree.
    pub(crate) expanded_zones: HashSet<String>,
    /// Which material/part headers are expanded, keyed by (zone, material).
    pub(crate) expanded_parts: HashSet<(String, String)>,
    /// Scroll position of the visibility popover's tree, kept across opens.
    pub(crate) popover_scroll: ScrollHandle,
    /// What a background reload re-exports. `None` until the pane's first
    /// ship load completes.
    reload_source: Option<ReloadSource>,
    /// The incoming-fire log's verdict beside each landed shell, worded once
    /// per comparison from [`ArmorScene::sim_verdict`] rather than per frame.
    sim_labels: HashMap<ShotId, SimAgreement>,
    /// The pane this viewport is one of. Weak: the pane owns the dock that
    /// owns this. Carried so the viewport's context menu can offer the pane's
    /// own controls -- the options rail and the penetration checker.
    pane: Option<WeakEntity<ArmorViewerPane>>,
    /// The display section's slider state.
    pub(crate) display_sliders: DisplaySettingsSliders,
    /// Kept alive so `display_sliders`' change subscriptions keep firing;
    /// replaced wholesale whenever the sliders are rebuilt.
    _display_slider_subscriptions: Vec<Subscription>,
    /// The display-settings popover's lighting slider state.
    pub(crate) lighting_sliders: LightingSliders,
    /// The ambient and key light colours.
    pub(crate) lighting_colors: LightingColors,
    _lighting_slider_subscriptions: Vec<Subscription>,
    _lighting_color_subscriptions: Vec<Subscription>,
    /// The export-confirm panel's target, if open: a snapshot taken when the
    /// panel opens, so a sidebar click while it is open cannot retarget an
    /// already-open "Export {A}?" confirmation onto ship B.
    export_confirm: Option<ExportConfirm>,
}

/// [`ViewportView::export_confirm`]'s snapshot, fixed for the lifetime of the
/// confirm panel.
struct ExportConfirm {
    bundle: Arc<ArmorAssetsBundle>,
    param_index: String,
    display_name: String,
    hull_lod: usize,
    selected_hull: Option<String>,
    selected_modules: HashMap<ComponentType, String>,
}

/// A light's colour as the picker takes it.
///
/// The shader works in linear channels and the picker in the colours a reader
/// sees, which is the same distinction `gpui_kit::rgb` makes.
fn rgb_color(channels: [f32; 3]) -> gpui_kit::Hsla {
    gpui_kit::Rgba { r: channels[0], g: channels[1], b: channels[2], a: 1.0 }.into()
}

/// The same the other way, for what a picker reports back.
fn color_rgb(color: gpui_kit::Hsla) -> [f32; 3] {
    let rgba: gpui_kit::Rgba = color.into();
    [rgba.r, rgba.g, rgba.b]
}

impl EventEmitter<ViewportChange> for ViewportView {}

impl ViewportView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();
        let scene = ArmorScene::new();
        let (display_sliders, display_slider_subscriptions) = Self::build_display_sliders(cx, &scene);
        let lighting = scene.lighting().clone();
        let (lighting_sliders, lighting_slider_subscriptions) = Self::build_lighting_sliders(cx, &lighting);
        let (lighting_colors, lighting_color_subscriptions) = Self::build_lighting_colors(window, cx, &lighting);
        Self {
            focus_handle,
            scene,
            gpu_failure: None,
            last_bounds: None,
            image: None,
            ticking: false,
            ship_loading: None,
            expanded_hull_groups: HashSet::new(),
            expanded_camo_groups: HashSet::new(),
            expanded_zones: HashSet::new(),
            expanded_parts: HashSet::new(),
            popover_scroll: ScrollHandle::new(),
            reload_source: None,
            sim_labels: HashMap::new(),
            pane: None,
            display_sliders,
            _display_slider_subscriptions: display_slider_subscriptions,
            lighting_sliders,
            lighting_colors,
            _lighting_color_subscriptions: lighting_color_subscriptions,
            _lighting_slider_subscriptions: lighting_slider_subscriptions,
            export_confirm: None,
        }
    }

    /// The scene this view presents, for the panels that read it.
    pub(crate) fn scene(&self) -> &ArmorScene {
        &self.scene
    }

    /// Runs `change` against the scene, then reports what it changed.
    pub(crate) fn update_scene<R>(&mut self, cx: &mut Context<Self>, change: impl FnOnce(&mut ArmorScene) -> R) -> R {
        let result = change(&mut self.scene);
        self.apply_scene_changes(cx);
        result
    }

    /// Emits what the scene reports as changed, keeps the ticker running
    /// while the scene has motion to apply, and repaints.
    fn apply_scene_changes(&mut self, cx: &mut Context<Self>) {
        let changes = self.scene.take_changes();
        if changes.camera {
            cx.emit(ViewportChange::Camera);
        }
        if changes.settings {
            cx.emit(ViewportChange::Settings);
        }
        if changes.trajectory_mode {
            cx.emit(ViewportChange::TrajectoryMode);
        }
        self.start_ticker(cx);
        if changes.any() || self.scene.is_dirty() {
            cx.notify();
        }
    }

    /// Ticks the scene at [`TICK_HZ`] for as long as it wants ticks: held
    /// movement keys and the gizmo snap animation. Stops itself once neither
    /// is left, so there is no idle cost.
    fn start_ticker(&mut self, cx: &mut Context<Self>) {
        if self.ticking || !self.scene.wants_tick() {
            return;
        }
        self.ticking = true;
        cx.spawn(async move |this, cx| {
            let mut last_tick = Instant::now();
            loop {
                cx.background_executor().timer(Duration::from_secs_f32(1.0 / TICK_HZ)).await;
                let now = Instant::now();
                let dt = now - last_tick;
                last_tick = now;
                let more = this.update(cx, |this, cx| {
                    this.scene.tick(dt);
                    // Cleared in the same update that sees the last tick, so
                    // input arriving after it starts a new ticker.
                    let more = this.scene.wants_tick();
                    if !more {
                        this.ticking = false;
                    }
                    this.apply_scene_changes(cx);
                    more
                });
                if !matches!(more, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
    }

    /// Creates a `SliderState` entity seeded to `default` (min/max/step).
    fn new_slider(cx: &mut Context<Self>, min: f32, max: f32, step: f32, default: f32) -> Entity<SliderState> {
        cx.new(|_| SliderState::new().min(min).max(max).step(step).default_value(default))
    }

    /// Subscribes so every `SliderEvent::Change` on `state` (fired
    /// continuously while the user drags the thumb, matching egui's own
    /// per-frame `Slider::changed()`) invokes `on_change` with the slider's
    /// current value.
    fn subscribe_slider(
        cx: &mut Context<Self>,
        state: &Entity<SliderState>,
        on_change: impl Fn(&mut ArmorScene, f32) + 'static,
    ) -> Subscription {
        cx.subscribe(state, move |this, _state, event, cx| {
            if let SliderEvent::Change(value) = event {
                this.update_scene(cx, |scene| on_change(scene, value.end()));
            }
        })
    }

    /// Builds [`DisplaySettingsSliders`] seeded from the scene. Called from
    /// `new()` and again after `apply_armor_defaults` reseeds it.
    fn build_display_sliders(
        cx: &mut Context<Self>,
        scene: &ArmorScene,
    ) -> (DisplaySettingsSliders, Vec<Subscription>) {
        let display = scene.display_settings();
        let waterline_opacity = Self::new_slider(cx, 0.05, 1.0, 0.01, display.waterline_opacity);
        let armor_opacity = Self::new_slider(cx, 0.1, 1.0, 0.01, display.armor_opacity);
        let marker_opacity = Self::new_slider(cx, 0.0, 1.0, 0.01, display.marker_opacity);
        let perspective_fov = Self::new_slider(cx, 30.0, 120.0, 1.0, CameraPerspective::default().fov_deg);
        // The range the egui slider uses: a hull heels this far in a hard
        // turn, and further than that reads as a capsize rather than a
        // camera angle worth checking armor against.
        let model_roll_deg = Self::new_slider(cx, -scene::ROLL_LIMIT_DEG, scene::ROLL_LIMIT_DEG, 0.5, 0.0);
        // The ranges the trajectory itself is resolved over.
        let camera_fov = Self::new_slider(cx, 0.0, 1.0, 0.01, 0.0);
        let camera_height = Self::new_slider(cx, -1.0, 1.0, 0.01, 0.0);
        let cast_range = Self::new_slider(
            cx,
            scene::CAST_RANGE_MIN,
            scene::CAST_RANGE_MAX,
            scene::CAST_RANGE_STEP,
            scene::DEFAULT_CAST_RANGE.value(),
        );
        let subs = vec![
            Self::subscribe_slider(cx, &waterline_opacity, |scene, v| {
                scene.mutate_display_settings(|d| d.waterline_opacity = v)
            }),
            Self::subscribe_slider(cx, &armor_opacity, |scene, v| {
                scene.mutate_display_settings(|d| d.armor_opacity = v)
            }),
            Self::subscribe_slider(cx, &marker_opacity, |scene, v| {
                scene.mutate_display_settings(|d| d.marker_opacity = v)
            }),
            Self::subscribe_slider(cx, &perspective_fov, |scene, v| scene.set_perspective_fov(v)),
            Self::subscribe_slider(cx, &model_roll_deg, |scene, v| scene.set_model_roll_deg(v)),
            Self::subscribe_slider(cx, &camera_fov, |scene, v| {
                let next = CameraRingSettings { fov: v, ..scene.camera_rings().clone() };
                scene.set_camera_rings(next);
            }),
            Self::subscribe_slider(cx, &camera_height, |scene, v| {
                let next = CameraRingSettings { height: v, ..scene.camera_rings().clone() };
                scene.set_camera_rings(next);
            }),
            Self::subscribe_slider(cx, &cast_range, |scene, v| scene.set_cast_range(Km::new(v))),
        ];
        (
            DisplaySettingsSliders {
                waterline_opacity,
                armor_opacity,
                marker_opacity,
                perspective_fov,
                model_roll_deg,
                camera_fov,
                camera_height,
                cast_range,
            },
            subs,
        )
    }

    /// The two colour pickers, seeded from `lighting` and reporting every change
    /// back into the scene.
    fn build_lighting_colors(
        window: &mut Window,
        cx: &mut Context<Self>,
        lighting: &LightingSettings,
    ) -> (LightingColors, Vec<Subscription>) {
        use gpui_kit::component::color_picker::ColorPickerState;

        let flat = cx.new(|cx| ColorPickerState::new(window, cx).default_value(rgb_color(lighting.flat_color)));
        let key = cx.new(|cx| ColorPickerState::new(window, cx).default_value(rgb_color(lighting.key_color)));
        let subs = vec![
            Self::subscribe_color(cx, &flat, |lighting, color| lighting.flat_color = color),
            Self::subscribe_color(cx, &key, |lighting, color| lighting.key_color = color),
        ];
        (LightingColors { flat, key }, subs)
    }

    /// Folds a picker's changes into the lighting it belongs to.
    ///
    /// A cleared picker is left alone: the shader has no meaning for "no colour",
    /// and the reader clearing the field is not asking for black.
    fn subscribe_color(
        cx: &mut Context<Self>,
        picker: &Entity<gpui_kit::component::color_picker::ColorPickerState>,
        apply: fn(&mut LightingSettings, [f32; 3]),
    ) -> Subscription {
        cx.subscribe(picker, move |this, _picker, event, cx| {
            let gpui_kit::component::color_picker::ColorPickerEvent::Change(Some(color)) = event else { return };
            let channels = color_rgb(*color);
            this.update_scene(cx, |scene| scene.mutate_lighting(|lighting| apply(lighting, channels)));
        })
    }

    /// Builds [`LightingSliders`] seeded from `lighting`. Called from `new()`
    /// and, to move every thumb at once, again from `set_lighting_preset`.
    fn build_lighting_sliders(
        cx: &mut Context<Self>,
        lighting: &LightingSettings,
    ) -> (LightingSliders, Vec<Subscription>) {
        let flat_intensity = Self::new_slider(cx, 0.0, 1.5, 0.01, lighting.flat_intensity);
        let key_intensity = Self::new_slider(cx, 0.0, 1.5, 0.01, lighting.key_intensity);
        let azimuth_deg = Self::new_slider(cx, 0.0, 360.0, 1.0, lighting.azimuth_deg);
        let elevation_deg = Self::new_slider(cx, -90.0, 90.0, 1.0, lighting.elevation_deg);
        let rim_strength = Self::new_slider(cx, 0.0, 1.0, 0.01, lighting.rim_strength);
        let specular_strength = Self::new_slider(cx, 0.0, 1.0, 0.01, lighting.specular_strength);
        let shininess = Self::new_slider(cx, 1.0, 128.0, 1.0, lighting.shininess);
        let subs = vec![
            Self::subscribe_slider(cx, &flat_intensity, |scene, v| scene.mutate_lighting(|l| l.flat_intensity = v)),
            Self::subscribe_slider(cx, &key_intensity, |scene, v| scene.mutate_lighting(|l| l.key_intensity = v)),
            Self::subscribe_slider(cx, &azimuth_deg, |scene, v| scene.mutate_lighting(|l| l.azimuth_deg = v)),
            Self::subscribe_slider(cx, &elevation_deg, |scene, v| scene.mutate_lighting(|l| l.elevation_deg = v)),
            Self::subscribe_slider(cx, &rim_strength, |scene, v| scene.mutate_lighting(|l| l.rim_strength = v)),
            Self::subscribe_slider(cx, &specular_strength, |scene, v| {
                scene.mutate_lighting(|l| l.specular_strength = v)
            }),
            Self::subscribe_slider(cx, &shininess, |scene, v| scene.mutate_lighting(|l| l.shininess = v)),
        ];
        (
            LightingSliders {
                flat_intensity,
                key_intensity,
                azimuth_deg,
                elevation_deg,
                rim_strength,
                specular_strength,
                shininess,
            },
            subs,
        )
    }

    /// Seeds the scene from the persisted `armor_viewer_defaults` row and
    /// rebuilds the display sliders so their thumbs reflect it. Called once
    /// from `ArmorViewerPane::apply_armor_defaults`.
    pub fn apply_armor_defaults(&mut self, defaults: Option<&ArmorViewerDefaultsRow>, cx: &mut Context<Self>) {
        self.scene.apply_defaults(defaults);
        let (display_sliders, subs) = Self::build_display_sliders(cx, &self.scene);
        self.display_sliders = display_sliders;
        self._display_slider_subscriptions = subs;
        self.apply_scene_changes(cx);
    }

    /// The assets this pane's ship was loaded from, which is where the
    /// translations for part and material names come from.
    pub fn reload_bundle(&self) -> Option<Arc<ArmorAssetsBundle>> {
        self.reload_source.as_ref().map(|source| Arc::clone(&source.bundle))
    }

    /// The class of the ship shown, which picks the game's class-specific
    /// plate names.
    pub(crate) fn shown_species(&self) -> Option<Species> {
        self.scene.current_armor().and_then(|armor| armor.species)
    }

    /// `material` named the way the game names it on the ship shown, or by
    /// its English label while the build is still loading.
    pub(crate) fn material_label(&self, material: ArmorMaterial) -> String {
        match self.reload_bundle() {
            Some(bundle) => material.localized_name(bundle.assets.metadata(), self.shown_species()),
            None => material.english_label(),
        }
    }

    /// Records which ship a later reload re-exports. Called by
    /// `ArmorViewerPane` alongside every `show_armor` call.
    pub fn set_reload_source(&mut self, bundle: Arc<ArmorAssetsBundle>, param_index: String, display_name: String) {
        self.reload_source = Some(ReloadSource { bundle, param_index, display_name });
    }

    /// This pane's reload source (bundle/param index/display name), if a ship
    /// has ever loaded here. `pane.rs`'s Compare-clone seeds a new pane's own
    /// `set_reload_source` from it.
    pub(crate) fn reload_source(&self) -> Option<(Arc<ArmorAssetsBundle>, String, String)> {
        self.reload_source.as_ref().map(|s| (Arc::clone(&s.bundle), s.param_index.clone(), s.display_name.clone()))
    }

    /// The param index of the ship actually shown right now. Gated on the
    /// scene holding armor (not just `reload_source`) so a load still in
    /// flight does not count as loaded for `pane.rs`'s re-click guard.
    pub(crate) fn loaded_param_index(&self) -> Option<&str> {
        self.scene.has_armor().then(|| self.reload_source.as_ref().map(|s| s.param_index.as_str())).flatten()
    }

    /// Sets the camera with no `ViewportChange::Camera` emit, for the
    /// receiving side of a camera mirror and the Compare-clone.
    pub(crate) fn set_camera(
        &mut self,
        camera: wows_toolkit_armor::viewport::camera::ArcballCamera,
        cx: &mut Context<Self>,
    ) {
        self.update_scene(cx, |scene| scene.set_camera(camera));
    }

    /// Applies another pane's settings with no `ViewportChange::Settings`
    /// emit and no undo entry.
    pub(crate) fn apply_synced(&mut self, settings: &scene::SyncedSettings, cx: &mut Context<Self>) {
        self.update_scene(cx, |scene| scene.apply_synced(settings));
    }

    /// Shows `armor`, collapsing every popover tree, since its rows belong
    /// to the previous ship.
    pub fn show_armor(&mut self, armor: Arc<LoadedShipArmor>, cx: &mut Context<Self>) {
        self.expanded_zones.clear();
        self.expanded_parts.clear();
        self.expanded_hull_groups.clear();
        self.expanded_camo_groups.clear();
        self.update_scene(cx, |scene| scene.show_armor(armor));
    }

    /// Adopts the pane's shared wgpu device once it finishes initializing,
    /// uploading a ship picked before it was ready.
    pub(crate) fn set_gpu(&mut self, ctx: Arc<GpuContext>, pipeline: Arc<GpuPipeline>, cx: &mut Context<Self>) {
        let had_armor = self.scene.has_armor();
        self.gpu_failure = None;
        self.scene.set_gpu(Gpu { ctx, pipeline });
        // A comparison asked for before the device arrived found no hull and
        // did nothing, so it is run again against the one just uploaded.
        if !had_armor
            && self.scene.has_armor()
            && let Some((taken, shells)) = self.pane().and_then(|pane| {
                let loaded = self.loaded_param_index().map(str::to_owned);
                pane.read(cx).incoming().to_compare(loaded.as_deref())
            })
        {
            self.compare_incoming(&taken, &shells, cx);
        }
        self.apply_scene_changes(cx);
    }

    /// Sends each landed shell through this hull, compares the result with
    /// what the server said, and words the verdicts for the log.
    pub(crate) fn compare_incoming(
        &mut self,
        taken: &[PreExtractedHit],
        shells: &HashMap<GameParamId, ShellInfo>,
        cx: &mut Context<Self>,
    ) {
        self.scene.compare_incoming(taken, shells);
        let scene = &self.scene;
        self.sim_labels = taken
            .iter()
            .filter_map(|taken| {
                let shot = taken.hit.hit.shot_id;
                scene.sim_verdict(shot).map(|verdict| (shot, super::analysis::agreement(verdict)))
            })
            .collect();
        self.apply_scene_changes(cx);
        cx.notify();
    }

    /// What this app's own simulation made of a landed shell, worded.
    pub(crate) fn sim_agreement(&self, shot: ShotId) -> Option<&SimAgreement> {
        self.sim_labels.get(&shot)
    }

    /// What a replay viewer reads beside the hull: how much of the ship was
    /// left, and how much of it had been hit.
    pub(crate) fn hit_readout(&self) -> Option<String> {
        let health = self.scene.hit_health()?;
        Some(
            t!("ui.armor.realtime_readout", health = format!("{health:.0}"), hits = self.scene.hits_drawn())
                .into_owned(),
        )
    }

    /// Adopts the pane this viewport belongs to.
    pub(crate) fn set_pane(&mut self, pane: WeakEntity<ArmorViewerPane>) {
        self.pane = Some(pane);
    }

    /// The pane this viewport belongs to, while it is still alive.
    pub(crate) fn pane(&self) -> Option<Entity<ArmorViewerPane>> {
        self.pane.as_ref().and_then(|pane| pane.upgrade())
    }

    /// The last plate the pointer was over: its zone and its thickness.
    pub(crate) fn last_plate(&self) -> Option<(SharedString, f32)> {
        self.scene.last_plate().map(|(zone, thickness)| (SharedString::from(zone.to_owned()), thickness))
    }

    pub(crate) fn shown_ship_name(&self) -> Option<SharedString> {
        self.reload_source.as_ref().map(|source| SharedString::from(source.display_name.clone()))
    }

    /// The ship being loaded into this viewport, if one is. Driven by the
    /// pane, which owns the load itself.
    pub(crate) fn set_ship_loading(&mut self, loading: Option<SharedString>, cx: &mut Context<Self>) {
        if self.ship_loading == loading {
            return;
        }
        self.ship_loading = loading;
        cx.notify();
    }

    /// Adopts the pane's shared-device init failure: shows `reason` instead
    /// of the viewport.
    pub(crate) fn set_gpu_failed(&mut self, reason: String, cx: &mut Context<Self>) {
        self.gpu_failure = Some(reason);
        cx.notify();
    }

    /// Re-renders the scene and rebuilds the displayed `RenderImage` when it
    /// is dirty and the container's size is known, dropping the previous
    /// frame's atlas tile.
    fn redraw_if_needed(&mut self, window: &mut Window) {
        let Some(bounds) = self.last_bounds else { return };
        let width = bounds.size.width.as_f32().round() as u32;
        let height = bounds.size.height.as_f32().round() as u32;
        let Some((w, h, rgba)) = self.scene.render((width, height)) else { return };
        let new_image = readback_to_render_image(w, h, rgba);
        if let Some(old) = self.image.take() {
            let _ = window.drop_image(old);
        }
        self.image = Some(new_image);
    }

    fn handle_mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus_handle, cx);
        let Some(button) = pointer_button(event.button) else { return };
        let at = point_to_vec2(event.position);
        self.update_scene(cx, |scene| scene.pointer_down(at, button, event.click_count as u32));
    }

    fn handle_mouse_move(&mut self, event: &MouseMoveEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let at = point_to_vec2(event.position);
        self.update_scene(cx, |scene| scene.pointer_moved(at));
    }

    fn handle_mouse_up(&mut self, event: &MouseUpEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(button) = pointer_button(event.button) else { return };
        let at = point_to_vec2(event.position);
        self.update_scene(cx, |scene| scene.pointer_up(at, button, event.modifiers.shift));
    }

    fn handle_mouse_up_out(&mut self, _event: &MouseUpEvent, _window: &mut Window, cx: &mut Context<Self>) {
        self.update_scene(cx, |scene| scene.pointer_cancel());
    }

    fn handle_scroll_wheel(&mut self, event: &ScrollEvent, window: &mut Window, cx: &mut Context<Self>) {
        let dy = event.delta.pixel_delta(window.line_height()).y.as_f32();
        self.update_scene(cx, |scene| scene.wheel(dy));
    }

    /// Ctrl/Cmd+S opens and closes the options rail; every other key is the
    /// scene's. An OS auto-repeat is skipped: the scene moves a held key on
    /// its own ticks, and a repeated command would toggle it back.
    fn handle_key_down(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if event.is_held {
            return;
        }
        let modifiers = event.keystroke.modifiers;
        let command = modifiers.secondary(cx);
        let key = event.keystroke.key.as_str();
        let asked = self.update_scene(cx, |scene| scene.key_down(key, command, modifiers.shift));
        if let Some(PaneCommand::ToggleOptions) = asked
            && let Some(pane) = self.pane()
        {
            pane.update(cx, |pane, cx| pane.toggle_options(cx));
        }
    }

    fn handle_key_up(&mut self, event: &KeyUpEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let key = event.keystroke.key.as_str();
        self.update_scene(cx, |scene| scene.key_up(key));
    }

    /// Toggles a zone header's expanded/collapsed state in the popover tree.
    pub(crate) fn toggle_zone_expanded(&mut self, zone: String, cx: &mut Context<Self>) {
        if !self.expanded_zones.remove(&zone) {
            self.expanded_zones.insert(zone);
        }
        cx.notify();
    }

    /// Toggles a material/part header's expanded/collapsed state in the
    /// popover tree.
    pub(crate) fn toggle_part_expanded(&mut self, key: (String, String), cx: &mut Context<Self>) {
        if !self.expanded_parts.remove(&key) {
            self.expanded_parts.insert(key);
        }
        cx.notify();
    }

    /// Toggles a hull-part-group header's expanded/collapsed state in the
    /// hull-visibility popover tree.
    pub(crate) fn toggle_hull_group_expanded(&mut self, group: String, cx: &mut Context<Self>) {
        if !self.expanded_hull_groups.remove(&group) {
            self.expanded_hull_groups.insert(group);
        }
        cx.notify();
    }

    /// Toggles a camo-origin group header's expanded/collapsed state in the
    /// hull popover's camo picker.
    pub(crate) fn toggle_camo_group_expanded(&mut self, group: String, cx: &mut Context<Self>) {
        if !self.expanded_camo_groups.remove(&group) {
            self.expanded_camo_groups.insert(group);
        }
        cx.notify();
    }

    /// Selects (or clears, `id: None`) the active camo scheme. A scheme is
    /// decoded on the background executor; the hull keeps the camo it has
    /// until the new one lands.
    pub(crate) fn select_camo(&mut self, id: Option<CamoSchemeId>, cx: &mut Context<Self>) {
        let Some(job) = self.update_scene(cx, |scene| scene.select_camo(id)) else { return };
        cx.spawn(async move |this, cx| {
            let result = cx.background_spawn(async move { job.run() }).await;
            let _ = this.update(cx, |this, cx| this.update_scene(cx, |scene| scene.apply_camo(result)));
        })
        .detach();
    }

    /// Hull-upgrade selector (`popover.rs`): switches hull and reloads.
    pub(crate) fn select_hull_upgrade(&mut self, key: String, cx: &mut Context<Self>) {
        if self.update_scene(cx, |scene| scene.select_hull_upgrade(key)) {
            self.reload_ship(cx);
        }
    }

    /// LOD selector (`popover.rs`): switches LOD and reloads.
    pub(crate) fn select_hull_lod(&mut self, lod: usize, cx: &mut Context<Self>) {
        if self.update_scene(cx, |scene| scene.select_hull_lod(lod)) {
            self.reload_ship(cx);
        }
    }

    /// Module-alternative selector (`popover.rs`): overrides `ct`'s
    /// component and reloads.
    pub(crate) fn select_module_alternative(&mut self, ct: ComponentType, name: String, cx: &mut Context<Self>) {
        if self.update_scene(cx, |scene| scene.select_module_alternative(ct, name)) {
            self.reload_ship(cx);
        }
    }

    /// Re-exports the current ship under the scene's hull/LOD/module
    /// selection on the background executor and hands it to the scene, which
    /// drops it if a later reload superseded it. The `LoadedShipArmor` is an
    /// immutable `Arc`, so every selector takes the full reload rather than
    /// the egui app's in-place hull-only and upgrade-only paths.
    fn reload_ship(&mut self, cx: &mut Context<Self>) {
        let Some(source) = &self.reload_source else { return };
        let bundle = Arc::clone(&source.bundle);
        let param_index = source.param_index.clone();
        let selection = self.scene.hull_selection();
        let options = match wows_toolkit_armor::load_ship::build_reload_options(
            &bundle.assets,
            &param_index,
            source.display_name.clone(),
            selection.lod,
            selection.hull,
            selection.modules,
        ) {
            Ok(options) => options,
            Err(e) => {
                tracing::error!("armor viewer: failed to build reload options: {e}");
                return;
            }
        };

        let generation = self.scene.begin_reload();
        let task = spawn_reload_ship_armor(bundle, param_index, options, cx);
        cx.spawn(async move |this, cx| {
            let armor = match task.await {
                Ok(armor) => Arc::new(armor),
                Err(e) => {
                    tracing::error!("armor viewer: failed to reload ship armor: {e}");
                    return;
                }
            };
            let _ = this.update(cx, |this, cx| this.update_scene(cx, |scene| scene.apply_reload(generation, armor)));
        })
        .detach();
    }

    /// Opens the export-confirm panel for this pane's displayed ship,
    /// snapshotting the reload source and the live hull/LOD/module selection
    /// so the export target is fixed for the panel's lifetime.
    pub(crate) fn open_export_confirm(&mut self, cx: &mut Context<Self>) {
        if !self.scene.has_armor() {
            return;
        }
        let Some(source) = &self.reload_source else { return };
        let selection = self.scene.hull_selection();
        self.export_confirm = Some(ExportConfirm {
            bundle: Arc::clone(&source.bundle),
            param_index: source.param_index.clone(),
            display_name: source.display_name.clone(),
            hull_lod: selection.lod,
            selected_hull: selection.hull,
            selected_modules: selection.modules,
        });
        cx.notify();
    }

    /// Dismisses the export-confirm panel, matching egui's own Cancel button
    /// (`tab.rs:1102-1104`).
    pub(crate) fn cancel_export(&mut self, cx: &mut Context<Self>) {
        self.export_confirm = None;
        cx.notify();
    }

    /// Confirms the export: closes the confirm panel, asks where to write
    /// `{display_name}.glb`, and exports the snapshot taken when the panel
    /// opened on the background executor. Reports the outcome in the words
    /// `ArmorViewerPane::export_model` uses for the same export.
    pub(crate) fn confirm_export(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(snapshot) = self.export_confirm.take() else { return };
        cx.notify();
        let asked = crate::dialog::save_file(
            None,
            &wows_toolkit_armor::load_ship::default_export_filename(&snapshot.display_name),
            Some(crate::dialog::GLB),
        );

        let bundle = snapshot.bundle;
        let param_index = snapshot.param_index;
        let display_name = snapshot.display_name;
        let options = wows_toolkit_armor::load_ship::export_options_from_selection(
            snapshot.hull_lod,
            snapshot.selected_hull,
            snapshot.selected_modules,
        );

        cx.spawn_in(window, async move |this, cx| {
            let Some(path) = asked.await else { return };
            let ship = display_name.clone();
            let written = cx
                .background_spawn(async move {
                    let result =
                        wows_toolkit_armor::load_ship::export_ship_glb(&bundle.assets, &param_index, &options, &path);
                    // The size is what the egui app reports beside the ship,
                    // and it is only knowable once the file is on disk.
                    result.map(|()| {
                        let size = std::fs::metadata(&path).map(|meta| meta.len()).ok();
                        (path, size)
                    })
                })
                .await;

            let _ = this.update_in(cx, |_this, window, cx| load_ship::report_export(written, &ship, window, cx));
        })
        .detach();
    }

    /// Applies a lighting preset (In-game/Flat/Studio popover buttons). A
    /// preset moves every lighting slider's thumb at once, so the sliders are
    /// rebuilt rather than just the domain value.
    pub(crate) fn set_lighting_preset(
        &mut self,
        preset: LightingSettings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.scene.set_lighting_preset(preset);
        let lighting = self.scene.lighting().clone();
        let (lighting_sliders, subs) = Self::build_lighting_sliders(cx, &lighting);
        self.lighting_sliders = lighting_sliders;
        self._lighting_slider_subscriptions = subs;
        // The pickers follow the preset rather than being rebuilt: a picker the
        // reader has open stays open, showing the colour the preset just set.
        let (flat, key) = (rgb_color(lighting.flat_color), rgb_color(lighting.key_color));
        self.lighting_colors.flat.update(cx, |picker, cx| picker.set_value(flat, window, cx));
        self.lighting_colors.key.update(cx, |picker, cx| picker.set_value(key, window, cx));
        self.apply_scene_changes(cx);
    }
}

impl Render for ViewportView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.redraw_if_needed(window);

        let status: Option<String> = match (&self.gpu_failure, self.scene.gpu_ready()) {
            (_, true) => None,
            (Some(reason), false) => Some(t!("ui.armor.init_failed", reason = reason).to_string()),
            (None, false) => Some(t!("ui.armor.initializing").into_owned()),
        };
        // Nothing is drawn until a ship is picked, so the viewport carries
        // the egui app's own two messages instead. A load in flight replaces
        // whatever is drawn, as it does in the egui pane, so the ship on
        // screen is never mistaken for the one being loaded.
        let loading_ship = self.ship_loading.clone();
        let empty_state = (!self.scene.has_armor() && loading_ship.is_none())
            .then(|| status.clone().unwrap_or_else(|| t!("ui.armor.select_ship").into_owned()));
        let image_child = match (loading_ship, empty_state, self.image.clone()) {
            (Some(name), _, _) => h_flex()
                .size_full()
                .items_center()
                .justify_center()
                .gap_2()
                .child(Spinner::new())
                .child(
                    div()
                        .text_sm()
                        .text_color(crate::theme::text_dim())
                        .child(t!("ui.armor.loading_named_ship", ship = name).to_string()),
                )
                .into_any_element(),
            (None, None, Some(image)) => img(image).object_fit(ObjectFit::Fill).size_full().into_any_element(),
            (None, message, _) => h_flex()
                .size_full()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .text_sm()
                        .text_color(crate::theme::text_dim())
                        .child(message.or(status).unwrap_or_else(|| t!("ui.armor.rendering").into_owned())),
                )
                .into_any_element(),
        };

        let gizmo_view = self.scene.gizmo();
        let weak = cx.weak_entity();
        let overlay = canvas(
            move |bounds: Bounds<Pixels>, _window, cx| {
                let _ = weak.update(cx, |this, cx| {
                    if this.last_bounds != Some(bounds) {
                        this.last_bounds = Some(bounds);
                        this.scene.set_rect(view_rect_from_bounds(bounds));
                        cx.notify();
                    }
                });
            },
            move |_bounds, (), window, cx| {
                if let Some(gizmo_view) = &gizmo_view {
                    draw_gizmo_overlay(gizmo_view, window, cx);
                }
            },
        )
        // `inset_0`, not `size_full`: an absolute element with no insets is
        // laid out at its static position, which is below the image it is
        // meant to cover.
        .absolute()
        .inset_0();

        // Floating tooltips, anchored near (not under) the cursor and snapped
        // back inside the window if they would overflow.
        let ring_overlay = self.scene.hovered_ring().map(|ring| {
            let theme = cx.theme();
            let cursor = vec2_to_point(ring.cursor);
            let anchor_pos = point(cursor.x + TOOLTIP_CURSOR_OFFSET, cursor.y + TOOLTIP_CURSOR_OFFSET);
            let element = div()
                .bg(theme.background)
                .border_1()
                .border_color(theme.border)
                .rounded(theme.radius)
                .px_2()
                .py_1()
                .text_xs()
                .children(ring.label.lines().map(|line| div().child(line.to_string())));
            deferred(anchored().position(anchor_pos).snap_to_window_with_margin(px(8.)).child(element))
                .with_priority(0)
                .into_any_element()
        });

        let tooltip_overlay = self.scene.hovered().filter(|_| self.scene.hovered_ring().is_none()).map(|hover| {
            let theme = cx.theme();
            let (background, border, radius, muted) =
                (theme.background, theme.border, theme.radius, theme.muted_foreground);
            // A zone and a plate are named the way the game names them, as the
            // egui tooltip does.
            let zone_label = match self.reload_bundle() {
                Some(bundle) => {
                    wows_toolkit_viewmodel::ship_catalog::translate_name(bundle.assets.metadata(), &hover.tooltip.zone)
                }
                None => hover.tooltip.zone.clone(),
            };
            let material_label = self.material_label(hover.tooltip.material);
            let element = picking_ui::tooltip_element(
                &hover.tooltip,
                background,
                border,
                radius,
                muted,
                &zone_label,
                &material_label,
            );
            let cursor = vec2_to_point(hover.cursor);
            let anchor_pos = point(cursor.x + TOOLTIP_CURSOR_OFFSET, cursor.y + TOOLTIP_CURSOR_OFFSET);
            deferred(anchored().position(anchor_pos).snap_to_window_with_margin(px(8.)).child(element))
                .with_priority(0)
                .into_any_element()
        });

        // Right-click context menu: hide/show the hovered plate, show all
        // hidden plates, and disable the hovered part's material. Snapshots
        // the hover and the plate visibility at render time, since the
        // menu-item click handlers only get `&mut App`.
        let entity = cx.entity();
        let hovered_for_menu = self.scene.hovered().cloned().map(|hover| {
            let label = self.material_label(hover.tooltip.material);
            (hover, label)
        });
        let plate_visibility = self.scene.plate_visibility();
        let hover_is_hidden =
            hovered_for_menu.as_ref().is_some_and(|(h, _)| plate_visibility.get(&h.key).copied().unwrap_or(false));
        let hidden_count = plate_visibility.len();

        // Export-confirm panel: a minimal inline overlay reproducing egui's
        // own `export_confirm` dialog (`tab.rs:1045-1111`). `.occlude()`
        // blocks the viewport's own mouse handlers underneath while open.
        let export_overlay = self.export_confirm.as_ref().map(|snapshot| {
            let theme = cx.theme();
            let (background, border, radius) = (theme.background, theme.border, theme.radius);
            let display_name = snapshot.display_name.clone();
            let cancel_entity = entity.clone();
            let confirm_entity = entity.clone();

            div()
                .id("armor-export-confirm-backdrop")
                .occlude()
                .absolute()
                .inset_0()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .bg(black().opacity(0.35))
                .child(
                    v_flex()
                        .id("armor-export-confirm-panel")
                        .w(px(360.))
                        .gap_3()
                        .p_4()
                        .bg(background)
                        .border_1()
                        .border_color(border)
                        .rounded(radius)
                        .shadow_md()
                        .child(div().font_weight(FontWeight::BOLD).child(t!("ui.armor.export_model").to_string()))
                        .child(div().text_sm().child(format!(
                            "Export {display_name} to a glTF Binary (.glb) file. These 3D models and textures are IP of Wargaming and any usage of these models should be in compliance with your local laws."
                        )))
                        .child(
                            h_flex()
                                .gap_2()
                                .justify_end()
                                .child(Button::new("armor-export-confirm-cancel").label(t!("ui.buttons.cancel").to_string()).compact().on_click(
                                    move |_, _window, cx| {
                                        cancel_entity.update(cx, |view, cx| view.cancel_export(cx));
                                    },
                                ))
                                .child(Button::new("armor-export-confirm-export").label(t!("ui.armor.export_button").to_string()).compact().on_click(
                                    move |_, window, cx| {
                                        confirm_entity.update(cx, |view, cx| view.confirm_export(window, cx));
                                    },
                                )),
                        ),
                )
                .into_any_element()
        });

        div()
            .id("armor-viewport")
            .track_focus(&self.focus_handle)
            .relative()
            .size_full()
            .overflow_hidden()
            .on_any_mouse_down(cx.listener(Self::handle_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::handle_mouse_up))
            .on_mouse_up(MouseButton::Middle, cx.listener(Self::handle_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::handle_mouse_up_out))
            .on_mouse_up_out(MouseButton::Middle, cx.listener(Self::handle_mouse_up_out))
            .on_mouse_move(cx.listener(Self::handle_mouse_move))
            .on_scroll(cx.listener(Self::handle_scroll_wheel))
            .on_key_down(cx.listener(Self::handle_key_down))
            .on_key_up(cx.listener(Self::handle_key_up))
            .child(image_child)
            .child(overlay)
            .children((self.scene.hits_drawn() > 0).then(|| {
                // Under the readout it belongs with, over the hull rather
                // than on the strip, which is the same whatever drives the
                // viewer.
                v_flex()
                    .absolute()
                    .top_10()
                    .left_2()
                    .gap_0p5()
                    .px_2()
                    .py_1()
                    .rounded(cx.theme().radius)
                    .bg(cx.theme().background.opacity(0.85))
                    .border_1()
                    .border_color(cx.theme().border)
                    .text_xs()
                    .children(self.scene.shot_log().into_iter().map(|line| {
                        h_flex()
                            .gap_2()
                            .child(div().text_color(cx.theme().muted_foreground).child(line.at))
                            .child(div().child(line.outcome))
                    }))
            }))
            .children(self.hit_readout().map(|readout| {
                // Over the hull rather than on the strip: it belongs to what
                // is being shown, and the strip is the same whatever drives
                // the viewer.
                div()
                    .absolute()
                    .top_2()
                    .left_2()
                    .px_2()
                    .py_1()
                    .rounded(cx.theme().radius)
                    .bg(cx.theme().background.opacity(0.85))
                    .border_1()
                    .border_color(cx.theme().border)
                    .text_xs()
                    .child(readout)
            }))
            .when_some(tooltip_overlay, |this, t| this.child(t))
            .when_some(ring_overlay, |this, t| this.child(t))
            .when_some(export_overlay, |this, o| this.child(o))
            // Right is not a camera button, so a press opens the menu without
            // competing with an orbit or a pan.
            .context_menu(move |mut menu, _window, cx| {
                let Some((hover, label)) = hovered_for_menu.clone() else {
                    return popover::quick_toggle_items(menu, &entity, cx);
                };
                let PlateKey { zone, material_name: material, thickness_tenths } = hover.key.clone();
                let thickness_mm = thickness_tenths as f32 / 10.0;

                let toggle_key = hover.key.clone();
                let toggle_entity = entity.clone();
                let toggle_label = if hover_is_hidden {
                    format!("Show {thickness_mm:.0} mm {label}")
                } else {
                    format!("Hide {thickness_mm:.0} mm {label}")
                };
                menu = menu.item(PopupMenuItem::new(toggle_label).on_click(move |_event, _window, cx| {
                    let key = toggle_key.clone();
                    toggle_entity.update(cx, |view, cx| view.update_scene(cx, |scene| scene.toggle_plate(key)));
                }));

                if hidden_count > 0 {
                    let show_all_entity = entity.clone();
                    menu = menu.item(PopupMenuItem::new(format!("Show all hidden plates ({hidden_count})")).on_click(
                        move |_event, _window, cx| {
                            show_all_entity
                                .update(cx, |view, cx| view.update_scene(cx, |scene| scene.show_all_plates()));
                        },
                    ));
                }

                menu = menu.separator();

                let disable_entity = entity.clone();
                let disable_zone = zone.clone();
                let disable_material_name = material;
                menu =
                    menu.item(PopupMenuItem::new(format!("Disable {label}")).on_click(move |_event, _window, cx| {
                        disable_entity.update(cx, |view, cx| {
                            view.update_scene(cx, |scene| scene.disable_material(&disable_zone, &disable_material_name))
                        });
                    }));

                popover::quick_toggle_items(menu.separator(), &entity, cx)
            })
    }
}

/// The scene's name for a gpui mouse button, or `None` for one the viewport
/// does not use.
fn pointer_button(button: MouseButton) -> Option<scene::Button> {
    match button {
        MouseButton::Left => Some(scene::Button::Primary),
        MouseButton::Middle => Some(scene::Button::Middle),
        MouseButton::Right => Some(scene::Button::Secondary),
        _ => None,
    }
}

fn view_rect_from_bounds(b: Bounds<Pixels>) -> ViewRect {
    ViewRect::new(b.origin.x.as_f32(), b.origin.y.as_f32(), b.size.width.as_f32(), b.size.height.as_f32())
}

fn point_to_vec2(p: Point<Pixels>) -> Vec2 {
    Vec2::new(p.x.as_f32(), p.y.as_f32())
}

fn vec2_to_point(v: Vec2) -> Point<Pixels> {
    point(px(v.x), px(v.y))
}

fn ball_bounds(center: Point<Pixels>, radius: f32) -> Bounds<Pixels> {
    let d = px(radius * 2.0);
    Bounds::new(point(center.x - px(radius), center.y - px(radius)), size(d, d))
}

/// Draws the six axis balls (line to tip + filled/stroked circle + X/Y/Z
/// label on the positive balls) in the scene's draw order, so the ball
/// nearest the camera draws on top, plus a white hover ring.
fn draw_gizmo_overlay(view: &GizmoView, window: &mut Window, cx: &mut App) {
    let center = vec2_to_point(view.center);
    for ball in &view.balls {
        let tip_point = vec2_to_point(ball.tip);
        let color: Hsla = rgb(scene::axis_rgb(ball.axis)).into();

        if ball.positive {
            let mut path = PathBuilder::stroke(px(2.0));
            path.move_to(center);
            path.line_to(tip_point);
            if let Ok(path) = path.build() {
                window.paint_path(path, color);
            }
            window.paint_quad(quad(
                ball_bounds(tip_point, gizmo::BALL_R),
                gizmo::BALL_R,
                color,
                0.,
                transparent_black(),
                BorderStyle::default(),
            ));
            paint_axis_label(scene::axis_label(ball.axis), tip_point, window, cx);
        } else {
            window.paint_quad(quad(
                ball_bounds(tip_point, gizmo::BALL_R),
                gizmo::BALL_R,
                transparent_black(),
                1.5,
                color,
                BorderStyle::default(),
            ));
        }

        if ball.hovered {
            window.paint_quad(quad(
                ball_bounds(tip_point, gizmo::BALL_R + 2.0),
                gizmo::BALL_R + 2.0,
                transparent_black(),
                2.,
                white(),
                BorderStyle::default(),
            ));
        }
    }
}

fn paint_axis_label(label: &'static str, tip: Point<Pixels>, window: &mut Window, cx: &mut App) {
    let run = TextRun {
        len: label.len(),
        font: window.text_style().font(),
        color: black(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let shaped = window.text_system().shape_line(label.into(), px(10.0), &[run], None);
    let origin = point(tip.x - shaped.width() * 0.5, tip.y - px(5.0));
    let _ = shaped.paint(origin, px(12.0), TextAlign::Left, None, window, cx);
}

#[cfg(test)]
mod tests {
    use gpui_kit::Bounds;
    use gpui_kit::point;
    use gpui_kit::px;
    use gpui_kit::size;

    use super::ball_bounds;
    use super::point_to_vec2;
    use super::vec2_to_point;
    use super::view_rect_from_bounds;

    #[test]
    fn view_rect_from_bounds_matches_origin_and_size() {
        let b = Bounds::new(point(px(10.0), px(20.0)), size(px(300.0), px(200.0)));
        let r = view_rect_from_bounds(b);
        assert_eq!((r.left(), r.top(), r.width(), r.height()), (10.0, 20.0, 300.0, 200.0));
    }

    #[test]
    fn point_vec2_round_trip() {
        let p = point(px(12.5), px(-4.0));
        let v = point_to_vec2(p);
        let back = vec2_to_point(v);
        assert_eq!((back.x.as_f32(), back.y.as_f32()), (12.5, -4.0));
    }

    #[test]
    fn ball_bounds_is_centered_square() {
        let center = point(px(100.0), px(50.0));
        let b = ball_bounds(center, 7.0);
        assert_eq!(b.size.width.as_f32(), 14.0);
        assert_eq!(b.size.height.as_f32(), 14.0);
        assert_eq!(b.origin.x.as_f32(), 93.0);
        assert_eq!(b.origin.y.as_f32(), 43.0);
    }
}
