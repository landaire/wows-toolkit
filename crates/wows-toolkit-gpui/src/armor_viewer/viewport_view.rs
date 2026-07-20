//! Armor viewport view: wraps the offscreen-rendered `img()` (Task 1) with
//! gpui mouse/scroll/key input driving the copied `viewport_3d` arcball
//! camera, plus a gpui-native redraw of the navigation gizmo overlaid on top.
//! Milestone 3 Task 6 adds CPU plate picking on top of the same mouse-move
//! handler: hovering a plate shows a thickness tooltip (`picking_ui.rs`) and
//! a highlight overlay; a plain click (not a camera drag) toggles that
//! plate's visibility; right-click opens a context menu with hide/show and
//! "disable material" actions. All of it shares this view's on-demand
//! render discipline -- picking is CPU-only and cheap, so it runs on every
//! mouse-move, but a highlight/geometry re-upload (GPU work) only happens
//! when the hovered plate or `plate_visibility` actually changes.
//!
//! Rendering is on-demand: `Viewport3D::is_dirty()` (set by any camera
//! mutation or mesh change) gates the expensive offscreen render + CPU
//! readback + `RenderImage` rebuild, done once per notified frame rather
//! than every frame. Hovering the gizmo only repaints the cheap CPU overlay
//! (no GPU work). The owned wgpu device is created on a background task so
//! it never blocks the UI thread; the view shows a brief status message
//! until it is ready.

use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use gpui::prelude::FluentBuilder;
use gpui::*;
use gpui_component::ActiveTheme;
use gpui_component::h_flex;
use gpui_component::menu::ContextMenuExt;
use gpui_component::menu::PopupMenuItem;
use gpui_component::slider::SliderEvent;
use gpui_component::slider::SliderState;
use gpui_component::v_flex;
use wows_toolkit_config::queries::ArmorViewerDefaultsRow;
use wowsunpack::export::camo_textures::CamoSchemeId;
use wowsunpack::export::camo_textures::SchemeTextures;
use wowsunpack::export::camouflage::UvTransform;
use wowsunpack::game_params::keys::ComponentType;

use crate::armor_viewer::assets::ArmorAssetsBundle;
use crate::armor_viewer::camo::build_active_camo;
use crate::armor_viewer::load_ship;
use crate::armor_viewer::load_ship::ArmorTriangleTooltip;
use crate::armor_viewer::load_ship::LoadedShipArmor;
use crate::armor_viewer::load_ship::PlateKey;
use crate::armor_viewer::load_ship::ShipLoadError;
use crate::armor_viewer::load_ship::spawn_reload_ship_armor;
use crate::armor_viewer::picking_ui;
use crate::armor_viewer::popover;
use crate::armor_viewer::upload;
use crate::armor_viewer::upload::upload_armor_to_viewport;
use crate::armor_viewer::upload_hull;
use crate::armor_viewer::visibility;
use crate::armor_viewer::visibility::SidebarHighlightKey;
use crate::armor_viewer::visibility::VisibilityFilter;
use crate::armor_viewer::visibility::VisibilitySnapshot;
use crate::armor_viewer::visibility::VisibilityUndoStack;
use crate::viewport::camera;
use crate::viewport::camera::ArcballCamera;
use crate::viewport::camera::Axis;
use crate::viewport::device::GpuContext;
use crate::viewport::device::readback_to_render_image;
use crate::viewport::device::unit_cube;
use crate::viewport::gizmo;
use crate::viewport::renderer::GpuPipeline;
use crate::viewport::renderer::LAYER_DEFAULT;
use crate::viewport::renderer::Viewport3D;
use crate::viewport::types::LightingSettings;
use crate::viewport::types::MeshId;
use crate::viewport::types::Vec2;
use crate::viewport::types::Vec3;
use crate::viewport::types::ViewRect;

/// Placeholder mesh color, uploaded once the owned wgpu device is ready.
/// Ship meshes replace this in a later milestone.
const PLACEHOLDER_COLOR: [f32; 4] = [0.35, 0.55, 0.85, 1.0];

/// How long a gizmo-snap animation runs before the ticker (`start_animation_ticker`)
/// stops re-rendering.
const GIZMO_SNAP_DURATION_SECS: f32 = 0.35;

/// Total pointer displacement (in pixels) since mouse-down before a drag
/// counts as a real drag rather than a click, matching egui's own drag
/// classification threshold. Below this, a mouse-up inside the gizmo box is
/// treated as a click (snap fires); at or above it, it's an orbit/pan.
const DRAG_THRESHOLD_PX: f32 = 4.0;

/// How often the held-key movement ticker (`start_key_ticker`) advances the
/// camera while a WASD/arrow key is held down.
const KEY_TICK_HZ: f32 = 60.0;

/// Offset (in each axis) from the cursor to the thickness tooltip's anchor
/// position, so the tooltip doesn't sit directly under the pointer.
const TOOLTIP_CURSOR_OFFSET: Pixels = px(16.);

#[derive(Clone, Copy, PartialEq, Eq)]
enum DragKind {
    Orbit,
    Pan,
}

#[derive(Clone, Copy)]
struct DragState {
    kind: DragKind,
    last_position: Point<Pixels>,
    /// Origin position at mouse-down, used to accumulate total displacement
    /// so small jitter doesn't get misclassified as a drag.
    start_position: Point<Pixels>,
    /// Whether accumulated displacement since mouse-down has exceeded
    /// `DRAG_THRESHOLD_PX`. Used to tell a plain click on the gizmo (snap)
    /// apart from a drag-orbit that started inside the gizmo box.
    moved: bool,
}

/// Movement keys tracked by the held-keys ticker for continuous camera motion.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum MoveKey {
    Forward,
    Back,
    Left,
    Right,
    Up,
    Down,
    RotateLeft,
    RotateRight,
}

impl MoveKey {
    fn from_str(key: &str) -> Option<Self> {
        match key {
            "w" => Some(Self::Forward),
            "s" => Some(Self::Back),
            "a" => Some(Self::Left),
            "d" => Some(Self::Right),
            "up" => Some(Self::Up),
            "down" => Some(Self::Down),
            "left" => Some(Self::RotateLeft),
            "right" => Some(Self::RotateRight),
            _ => None,
        }
    }
}

/// Lifecycle of the owned wgpu device backing this viewport.
enum GpuState {
    /// The background device-creation task is still running.
    Initializing,
    Ready {
        ctx: GpuContext,
        pipeline: GpuPipeline,
    },
    Failed(String),
}

/// The currently hovered armor plate (CPU picking result), if any: its key
/// (for click-to-hide and highlight matching), the tooltip content to show,
/// and the cursor position the tooltip is anchored near. Cloned into a
/// `.context_menu()` closure and a `cx.notify()`-triggered render pass, so it
/// is cheap to clone (a `PlateKey`, an `ArmorTriangleTooltip`, and a `Point`).
#[derive(Clone)]
struct HoverInfo {
    key: PlateKey,
    tooltip: ArmorTriangleTooltip,
    cursor: Point<Pixels>,
}

/// The display-settings popover's two numeric sliders (`popover.rs`),
/// backing `ViewportView::display_settings`'s `waterline_opacity`/
/// `armor_opacity` fields. A persistent `Entity<SliderState>` per slider so
/// its thumb position survives the popover closing and reopening (the
/// popover's `.content()` closure only runs while open, see `popover.rs`'s
/// module doc); rebuilt wholesale (not `set_value`d) whenever the seed
/// changes outside a drag (`apply_armor_defaults`), since `SliderState::
/// set_value` needs a `&mut Window` that isn't available there -- mirrors
/// `app.rs`'s own `zoom_slider` reseed-by-recreation.
pub(crate) struct DisplaySettingsSliders {
    pub(crate) waterline_opacity: Entity<SliderState>,
    pub(crate) armor_opacity: Entity<SliderState>,
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

/// What a Milestone 4 Task 8c background reload needs: the shared
/// ship-export bundle, this ship's param index, and its display name (all
/// otherwise owned by `ArmorViewerPane`, not this view). Set once via
/// [`ViewportView::set_reload_source`] whenever the pane loads a ship
/// (`pane.rs`'s `apply_ship_load_result`, right before `show_armor`); `None`
/// until the first ship load completes. A reload itself (`reload_ship`)
/// never mutates this -- only a genuinely new ship selection replaces it.
struct ReloadSource {
    bundle: Arc<ArmorAssetsBundle>,
    param_index: String,
    display_name: String,
}

pub struct ViewportView {
    focus_handle: FocusHandle,
    viewport: Viewport3D,
    gpu: GpuState,
    /// World-space bounding box used to frame the camera on reset/double-click.
    model_bounds: Option<(Vec3, Vec3)>,
    /// The container's window-space bounds as of the last paint. `None` until
    /// the first layout pass; the redraw and every screen-space projection
    /// (gizmo hit test, camera aspect) wait on it.
    last_bounds: Option<Bounds<Pixels>>,
    image: Option<Arc<RenderImage>>,
    drag: Option<DragState>,
    gizmo_hover: Option<(Axis, bool)>,
    /// Whether the currently-held left button went down inside the gizmo box,
    /// so a same-press release decides between a drag-orbit and a snap click.
    gizmo_press_in_box: bool,
    /// Whether the gizmo-snap animation ticker (`start_animation_ticker`) is
    /// currently running, so a second snap click does not stack a duplicate
    /// ticker advancing the animation twice as fast.
    animating: bool,
    /// Movement keys currently held down, driving the continuous-motion
    /// ticker (`start_key_ticker`) while non-empty.
    held_keys: HashSet<MoveKey>,
    /// Whether the held-key movement ticker is currently running, so a
    /// second key-down does not stack a duplicate ticker.
    key_ticking: bool,
    /// A ship picked in the sidebar before the owned wgpu device finished
    /// initializing. `apply_gpu_result` uploads this instead of the
    /// placeholder cube once the device becomes ready, then clears it.
    pending_armor: Option<Arc<LoadedShipArmor>>,
    /// The armor currently displayed, kept (beyond the initial upload) so a
    /// visibility change can re-upload without reloading the ship. `pub(crate)`
    /// so `popover.rs` can read it while building the popover's tree.
    pub(crate) current_armor: Option<Arc<LoadedShipArmor>>,
    /// Explicit part-level visibility overrides from the armor-visibility
    /// popover (`popover.rs`); absent = visible. Present value = is-visible
    /// (matches the egui app's own sense, unlike `plate_visibility` below --
    /// see `visibility.rs`'s module doc). Reset whenever a new ship loads.
    /// Mutations always go through `snapshot_and_mutate`. `pub(crate)` so
    /// `popover.rs` can read it while building the popover's tree.
    pub(crate) part_visibility: HashMap<(String, String), bool>,
    /// Explicitly-hidden plates (click-to-hide / context-menu toggle,
    /// `picking_ui.rs`, and the popover's plate rows); absent = visible.
    /// Present value = true means explicitly hidden (opposite sense from
    /// `part_visibility`, see `visibility.rs`'s module doc). Reset whenever a
    /// new ship loads. Mutations always go through `snapshot_and_mutate`.
    /// `pub(crate)` so `popover.rs` can read it while building the popover's tree.
    pub(crate) plate_visibility: HashMap<PlateKey, bool>,
    /// Per hull-mesh visibility overrides from the hull-visibility popover
    /// (`popover.rs`, Milestone 4 Task 8a): mesh name -> is-visible. Absent =
    /// HIDDEN (opposite default from `part_visibility`, matching the egui
    /// app's own `hull_visibility` default -- a freshly loaded ship shows no
    /// hull until the user turns parts on). Reset (cleared) whenever a new
    /// ship loads. `pub(crate)` so `popover.rs` can read it while building the
    /// hull popover's tree.
    pub(crate) hull_visibility: HashMap<String, bool>,
    /// Mesh ids of the currently-uploaded hull meshes, so
    /// [`Self::reupload_hull`] can remove the old set before re-adding the
    /// ones `hull_visibility` now marks visible. Ports the egui app's
    /// `pane.hull_mesh_ids`.
    hull_mesh_ids: Vec<MeshId>,
    /// Which hull-part-group headers are expanded in the hull-visibility
    /// popover's tree. Purely local UI state (not visibility, not
    /// undo-tracked); collapsed by default, matching egui's own
    /// `load_with_default_open(.., false)`. `pub(crate)` so `popover.rs` can
    /// read it while building the hull popover's tree.
    pub(crate) expanded_hull_groups: HashSet<String>,
    /// Which camo-origin group headers (Universal/Expendable/LegacyScan) are
    /// expanded in the hull-visibility popover's camo picker. Purely local UI
    /// state, same defaults/scope as `expanded_hull_groups`. `pub(crate)` so
    /// `popover.rs` can read it while building the camo picker.
    pub(crate) expanded_camo_groups: HashSet<String>,
    /// What a background reload (`reload_ship`) needs to re-export this same
    /// ship under a new selection. `None` until the pane's first ship load
    /// completes (`set_reload_source`); a reload never clears or replaces
    /// this itself.
    reload_source: Option<ReloadSource>,
    /// The hull-upgrade key selected in the hull popover (`popover.rs`),
    /// `None` = stock/default hull. Reset whenever a new ship loads. Threaded
    /// into `reload_ship`'s `ShipLoadOptions`. `pub(crate)` so `popover.rs`
    /// can read it while building the hull-upgrade selector.
    pub(crate) selected_hull: Option<String>,
    /// The hull LOD level selected in the hull popover, `0` = highest detail
    /// (matching `load_ship::DEFAULT_LOD`). Reset to `0` whenever a new ship
    /// loads. `pub(crate)` so `popover.rs` can read it while building the LOD
    /// selector.
    pub(crate) hull_lod: usize,
    /// Module-alternative overrides selected in the hull popover: component
    /// type -> chosen component name. Reset (cleared) whenever a new ship
    /// loads, or whenever `selected_hull` changes (alternatives differ per
    /// hull, matching the egui app's own selector, `tab.rs:3169`).
    /// `pub(crate)` so `popover.rs` can read it while building the
    /// module-alternative selectors.
    pub(crate) selected_modules: HashMap<ComponentType, String>,
    /// Bumped by every `reload_ship` call and captured into that reload's
    /// background task; a completing reload whose captured value no longer
    /// matches this field was superseded by a later selector change and its
    /// result is discarded. Mirrors `ArmorViewerPane::ship_load_generation`'s
    /// identical guard for ship selection, scoped to this view instead since
    /// a reload never touches the pane.
    reload_generation: u64,
    /// The currently selected camo scheme, if any (`None` = stock/base
    /// albedo). Set by [`Self::select_camo`], driven by the hull popover's
    /// camo picker (`popover.rs`). Reset (cleared) whenever a new ship loads.
    /// `pub(crate)` so `popover.rs` can read it while building the picker.
    pub(crate) selected_camo: Option<CamoSchemeId>,
    /// Cache of decoded camo scheme textures, keyed by scheme id, so
    /// reselecting a scheme (or the popover reopening) does not re-decode.
    /// Reset (cleared) whenever a new ship loads -- a cached decode from a
    /// previous ship is meaningless for a different ship's `CamoTextureSource`.
    camo_texture_cache: HashMap<CamoSchemeId, SchemeTextures>,
    /// The active camo's composited hull textures, keyed by mfm stem
    /// (`camo::build_active_camo`'s output). Empty when `selected_camo` is
    /// `None`. Lives here (not on `LoadedShipArmor`, an immutable `Arc`) and
    /// is passed into [`upload_hull::upload_hull_meshes`] as a parameter on
    /// every hull upload. Reset (cleared) whenever a new ship loads.
    active_camo_textures: HashMap<String, (u32, u32, Vec<u8>)>,
    /// The active camo's UV transforms, keyed by mfm stem, for stems the
    /// compositor left tiled (GPU-side) rather than baking. Same lifetime/
    /// reset rules as `active_camo_textures`.
    active_camo_uvs: HashMap<String, UvTransform>,
    /// Undo/redo history over `{part_visibility, plate_visibility}`. Every
    /// mutator snapshots the pre-mutation state here via `snapshot_and_mutate`
    /// before applying the change. Cleared whenever a new ship loads.
    undo_stack: VisibilityUndoStack,
    /// Which zone headers are expanded in the visibility popover's tree.
    /// Purely local UI state (not visibility, not undo-tracked); collapsed by
    /// default, matching egui's own `load_with_default_open(.., false)`.
    /// `pub(crate)` so `popover.rs` can read it while building the popover's tree.
    pub(crate) expanded_zones: HashSet<String>,
    /// Which material/part headers are expanded in the visibility popover's
    /// tree, keyed by (zone, material). Same defaults/scope as `expanded_zones`.
    pub(crate) expanded_parts: HashSet<(String, String)>,
    /// Scroll position of the visibility popover's zone/material/plate tree,
    /// persisted across opens so the popover doesn't reset to the top every
    /// time it's reopened. `pub(crate)` so `popover.rs` can clone a handle
    /// into its lazily-rebuilt content closure.
    pub(crate) popover_scroll: ScrollHandle,
    /// The currently hovered armor plate from CPU picking, if any.
    hovered: Option<HoverInfo>,
    /// The overlay mesh highlighting `hovered`'s plate, if any, so it can be
    /// removed/replaced when the hovered plate changes.
    hover_highlight: Option<(PlateKey, MeshId)>,
    /// The overlay mesh highlighting a hovered Zone/Part/Plate row in the
    /// visibility popover, if any. Tracked separately from `hover_highlight`
    /// (raycast-driven) so the two don't fight each other; cleared when the
    /// hovered row changes, the popover closes, or a visibility mutation
    /// re-uploads the armor.
    sidebar_highlight: Option<(SidebarHighlightKey, MeshId)>,
    /// Per-mesh triangle tooltip data from the most recent armor upload, used
    /// to map a `pick()` hit back to its `ArmorTriangleTooltip`.
    mesh_triangle_info: Vec<(MeshId, Vec<ArmorTriangleTooltip>)>,
    /// Live display settings (plate edges, waterline, zero-mm plates, armor
    /// opacity) mutable via the display-settings popover (`popover.rs`, Task
    /// 7b); changing any field re-uploads the armor since they affect
    /// geometry. Seeded from `ArmorViewerDefaults` (`apply_armor_defaults`),
    /// falling back to the same fixed values `upload.rs` used to bake in as
    /// constants. `pub(crate)` so `popover.rs` can read it while building
    /// both popovers' content -- the visibility popover's own zero-mm
    /// filtering (`popover.rs`) reads `display_settings.show_zero_mm` too, so
    /// toggling it here updates both the 3D geometry and which plate rows
    /// that popover's tree shows.
    pub(crate) display_settings: upload::DisplaySettings,
    /// The display-settings popover's waterline/armor opacity slider state.
    pub(crate) display_sliders: DisplaySettingsSliders,
    /// Kept alive so `display_sliders`' `SliderEvent::Change` subscriptions
    /// keep firing; replaced wholesale (dropping and canceling the old ones)
    /// whenever `apply_armor_defaults` rebuilds the sliders.
    _display_slider_subscriptions: Vec<Subscription>,
    /// The display-settings popover's lighting slider state.
    pub(crate) lighting_sliders: LightingSliders,
    /// Kept alive so `lighting_sliders`' `SliderEvent::Change` subscriptions
    /// keep firing; replaced wholesale whenever `set_lighting_preset` rebuilds
    /// the sliders.
    _lighting_slider_subscriptions: Vec<Subscription>,
}

impl ViewportView {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();
        let display_settings = upload::DisplaySettings::default();
        let (display_sliders, display_slider_subscriptions) = Self::build_display_sliders(cx, display_settings);
        let lighting = LightingSettings::default();
        let (lighting_sliders, lighting_slider_subscriptions) = Self::build_lighting_sliders(cx, &lighting);
        let mut this = Self {
            focus_handle,
            viewport: Viewport3D::new(),
            gpu: GpuState::Initializing,
            model_bounds: None,
            last_bounds: None,
            image: None,
            drag: None,
            gizmo_hover: None,
            gizmo_press_in_box: false,
            animating: false,
            held_keys: HashSet::new(),
            key_ticking: false,
            pending_armor: None,
            current_armor: None,
            part_visibility: HashMap::new(),
            plate_visibility: HashMap::new(),
            hull_visibility: HashMap::new(),
            hull_mesh_ids: Vec::new(),
            expanded_hull_groups: HashSet::new(),
            expanded_camo_groups: HashSet::new(),
            reload_source: None,
            selected_hull: None,
            hull_lod: load_ship::DEFAULT_LOD,
            selected_modules: HashMap::new(),
            reload_generation: 0,
            selected_camo: None,
            camo_texture_cache: HashMap::new(),
            active_camo_textures: HashMap::new(),
            active_camo_uvs: HashMap::new(),
            undo_stack: VisibilityUndoStack::default(),
            expanded_zones: HashSet::new(),
            expanded_parts: HashSet::new(),
            popover_scroll: ScrollHandle::new(),
            hovered: None,
            hover_highlight: None,
            sidebar_highlight: None,
            mesh_triangle_info: Vec::new(),
            display_settings,
            display_sliders,
            _display_slider_subscriptions: display_slider_subscriptions,
            lighting_sliders,
            _lighting_slider_subscriptions: lighting_slider_subscriptions,
        };
        this.start_gpu_init(cx);
        this
    }

    /// Creates a `SliderState` entity seeded to `default` (min/max/step), for
    /// [`build_display_sliders`](Self::build_display_sliders)/
    /// [`build_lighting_sliders`](Self::build_lighting_sliders).
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
        on_change: impl Fn(&mut Self, f32, &mut Context<Self>) + 'static,
    ) -> Subscription {
        cx.subscribe(state, move |this, _state, event, cx| {
            if let SliderEvent::Change(value) = event {
                on_change(this, value.end(), cx);
            }
        })
    }

    /// Builds [`DisplaySettingsSliders`] seeded from `display`, wired so a
    /// drag calls [`mutate_display_settings`](Self::mutate_display_settings).
    /// Called from `new()` and, to reseed after `apply_armor_defaults` loads
    /// a persisted row, again there.
    fn build_display_sliders(
        cx: &mut Context<Self>,
        display: upload::DisplaySettings,
    ) -> (DisplaySettingsSliders, Vec<Subscription>) {
        let waterline_opacity = Self::new_slider(cx, 0.05, 1.0, 0.01, display.waterline_opacity);
        let armor_opacity = Self::new_slider(cx, 0.1, 1.0, 0.01, display.armor_opacity);
        let subs = vec![
            Self::subscribe_slider(cx, &waterline_opacity, |this, v, cx| {
                this.mutate_display_settings(cx, |d| d.waterline_opacity = v)
            }),
            Self::subscribe_slider(cx, &armor_opacity, |this, v, cx| {
                this.mutate_display_settings(cx, |d| d.armor_opacity = v)
            }),
        ];
        (DisplaySettingsSliders { waterline_opacity, armor_opacity }, subs)
    }

    /// Builds [`LightingSliders`] seeded from `lighting`, wired so a drag
    /// calls [`mutate_lighting`](Self::mutate_lighting). Called from `new()`
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
            Self::subscribe_slider(cx, &flat_intensity, |this, v, cx| {
                this.mutate_lighting(cx, |l| l.flat_intensity = v)
            }),
            Self::subscribe_slider(cx, &key_intensity, |this, v, cx| this.mutate_lighting(cx, |l| l.key_intensity = v)),
            Self::subscribe_slider(cx, &azimuth_deg, |this, v, cx| this.mutate_lighting(cx, |l| l.azimuth_deg = v)),
            Self::subscribe_slider(cx, &elevation_deg, |this, v, cx| this.mutate_lighting(cx, |l| l.elevation_deg = v)),
            Self::subscribe_slider(cx, &rim_strength, |this, v, cx| this.mutate_lighting(cx, |l| l.rim_strength = v)),
            Self::subscribe_slider(cx, &specular_strength, |this, v, cx| {
                this.mutate_lighting(cx, |l| l.specular_strength = v)
            }),
            Self::subscribe_slider(cx, &shininess, |this, v, cx| this.mutate_lighting(cx, |l| l.shininess = v)),
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

    /// Seeds `display_settings` from the persisted `armor_viewer_defaults`
    /// row (falls back to the egui defaults when `None`, matching `upload::
    /// DisplaySettings::from_defaults`), rebuilds the two display sliders so
    /// their thumbs reflect the new seed, and -- if a ship is already shown
    /// -- re-uploads it. Called once from `ArmorViewerPane::
    /// apply_armor_defaults`, itself called once from `App::apply_settings`.
    pub fn apply_armor_defaults(&mut self, defaults: Option<&ArmorViewerDefaultsRow>, cx: &mut Context<Self>) {
        self.display_settings = upload::DisplaySettings::from_defaults(defaults);
        let (display_sliders, subs) = Self::build_display_sliders(cx, self.display_settings);
        self.display_sliders = display_sliders;
        self._display_slider_subscriptions = subs;
        self.reupload_current_armor(cx);
    }

    /// Records what a later `reload_ship` call needs to re-export this ship
    /// under a new hull/LOD/module selection. Called by `ArmorViewerPane`
    /// alongside every `show_armor` call (`pane.rs`'s `apply_ship_load_result`),
    /// including on a genuinely new ship selection -- `show_armor`'s own
    /// reset (`upload_armor_now`) is what actually clears the selection state
    /// itself; this only updates which ship a reload targets.
    pub fn set_reload_source(&mut self, bundle: Arc<ArmorAssetsBundle>, param_index: String, display_name: String) {
        self.reload_source = Some(ReloadSource { bundle, param_index, display_name });
    }

    /// Shows `armor`'s armor meshes in this viewport: clears any previous
    /// meshes, uploads the new ones (`upload::upload_armor_to_viewport`,
    /// which also frames the camera on the model's bounds and marks the
    /// viewport dirty), and updates `model_bounds` so double-click-to-reset
    /// frames the new ship. If the owned wgpu device has not finished
    /// initializing yet, stashes `armor` in `pending_armor` instead;
    /// `apply_gpu_result` uploads it as soon as the device becomes ready.
    pub fn show_armor(&mut self, armor: Arc<LoadedShipArmor>, cx: &mut Context<Self>) {
        if matches!(self.gpu, GpuState::Ready { .. }) {
            self.upload_armor_now(armor);
            self.pending_armor = None;
        } else {
            self.pending_armor = Some(armor);
        }
        cx.notify();
    }

    /// Uploads `armor` into the viewport and keeps it as `current_armor` for
    /// later visibility-only re-uploads (`reupload_current_armor`). Resets
    /// `part_visibility`/`plate_visibility`/`hull_visibility`, the hull/LOD/
    /// module selection (`selected_hull`/`hull_lod`/`selected_modules`), the
    /// undo/redo history, the popover's expanded-row state, and any hover
    /// state -- a freshly loaded ship starts with every armor plate visible,
    /// every hull mesh HIDDEN (matching the egui app's own `hull_visibility`
    /// default), stock hull at the highest-detail LOD, and nothing
    /// hovered/undoable, matching the egui app's own reset on ship load
    /// (`tab.rs:1453-1454`, `2393-2394`, `2741`). Uploads the hull too (a
    /// visual no-op the first time, since `hull_visibility` is empty, but
    /// exercises the same path a later hull-visibility toggle uses). Callers
    /// must have already checked `self.gpu` is `Ready`; a no-op otherwise.
    fn upload_armor_now(&mut self, armor: Arc<LoadedShipArmor>) {
        let GpuState::Ready { ctx, pipeline } = &self.gpu else { return };
        let device = ctx.device.clone();
        let queue = ctx.queue.clone();
        self.part_visibility.clear();
        self.plate_visibility.clear();
        self.hull_visibility.clear();
        self.hull_mesh_ids.clear();
        self.expanded_hull_groups.clear();
        self.expanded_camo_groups.clear();
        self.selected_hull = None;
        self.hull_lod = load_ship::DEFAULT_LOD;
        self.selected_modules.clear();
        self.selected_camo = None;
        self.camo_texture_cache.clear();
        self.active_camo_textures.clear();
        self.active_camo_uvs.clear();
        self.undo_stack.clear();
        self.expanded_zones.clear();
        self.expanded_parts.clear();
        self.hovered = None;
        self.hover_highlight = None;
        self.sidebar_highlight = None;
        let visibility = VisibilityFilter { part: &self.part_visibility, plate: &self.plate_visibility };
        self.mesh_triangle_info =
            upload_armor_to_viewport(&mut self.viewport, &device, &armor, visibility, self.display_settings);
        upload_hull::upload_hull_meshes(
            &mut self.viewport,
            &device,
            &queue,
            pipeline,
            &armor,
            &mut self.hull_mesh_ids,
            &self.hull_visibility,
            self.display_settings.hull_opaque,
            &self.active_camo_textures,
            &self.active_camo_uvs,
        );
        self.model_bounds = Some(armor.bounds);
        self.current_armor = Some(armor);
    }

    /// Creates the owned wgpu device off the UI thread (device/adapter
    /// negotiation blocks on `pollster` internally) and uploads the
    /// placeholder mesh once it lands back on the entity.
    fn start_gpu_init(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let created = cx
                .background_spawn(async move {
                    let ctx = GpuContext::new()?;
                    let pipeline = ctx.pipeline();
                    Ok::<_, anyhow::Error>((ctx, pipeline))
                })
                .await;
            let _ = this.update(cx, |this, cx| this.apply_gpu_result(created, cx));
        })
        .detach();
    }

    fn apply_gpu_result(&mut self, result: anyhow::Result<(GpuContext, GpuPipeline)>, cx: &mut Context<Self>) {
        match result {
            Ok((ctx, pipeline)) => {
                self.gpu = GpuState::Ready { ctx, pipeline };
                // A ship picked in the sidebar while the device was still
                // initializing takes priority over the placeholder cube.
                if let Some(armor) = self.pending_armor.take() {
                    self.upload_armor_now(armor);
                } else {
                    let GpuState::Ready { ctx, .. } = &self.gpu else { unreachable!() };
                    let (vertices, indices) = unit_cube(PLACEHOLDER_COLOR);
                    self.viewport.add_mesh(&ctx.device, &vertices, &indices, LAYER_DEFAULT);
                    let (min, max) = (Vec3::new(-0.5, -0.5, -0.5), Vec3::new(0.5, 0.5, 0.5));
                    self.viewport.camera = ArcballCamera::from_bounds(min, max);
                    self.model_bounds = Some((min, max));
                }
                self.viewport.mark_dirty();
            }
            Err(e) => {
                tracing::error!("armor viewport: failed to create owned wgpu device: {e:#}");
                self.gpu = GpuState::Failed(format!("{e:#}"));
            }
        }
        cx.notify();
    }

    /// Re-runs the offscreen render and rebuilds the displayed `RenderImage`
    /// if (and only if) the viewport is dirty and the container's size is
    /// known. Drops the previous frame's atlas tile before presenting the
    /// new one.
    fn redraw_if_needed(&mut self, window: &mut Window) {
        let GpuState::Ready { ctx, pipeline } = &self.gpu else { return };
        if !self.viewport.is_dirty() {
            return;
        }
        let Some(bounds) = self.last_bounds else { return };
        let width = (bounds.size.width.as_f32().round() as u32).max(1);
        let height = (bounds.size.height.as_f32().round() as u32).max(1);
        let Some((w, h, rgba)) =
            self.viewport.render_offscreen_rgba(&ctx.device, &ctx.queue, pipeline, (width, height))
        else {
            return;
        };
        let new_image = readback_to_render_image(w, h, rgba);
        if let Some(old) = self.image.take() {
            let _ = window.drop_image(old);
        }
        self.image = Some(new_image);
        self.viewport.clear_dirty();
    }

    fn gizmo_rect(&self) -> Option<ViewRect> {
        self.last_bounds.map(|b| gizmo::gizmo_rect(view_rect_from_bounds(b)))
    }

    fn viewport_size(&self) -> (f32, f32) {
        match self.last_bounds {
            Some(b) => (b.size.width.as_f32(), b.size.height.as_f32()),
            None => (1.0, 1.0),
        }
    }

    fn handle_mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus_handle, cx);
        if !self.is_gpu_ready() {
            return;
        }
        match event.button {
            MouseButton::Left => {
                let pointer = point_to_vec2(event.position);
                let in_gizmo = self.gizmo_rect().is_some_and(|r| r.contains(pointer));
                self.gizmo_press_in_box = in_gizmo;
                if !in_gizmo && event.click_count >= 2 {
                    if let Some((min, max)) = self.model_bounds {
                        self.viewport.camera.reset(min, max);
                        self.viewport.mark_dirty();
                        cx.notify();
                    }
                    self.drag = None;
                    return;
                }
                self.drag = Some(DragState {
                    kind: DragKind::Orbit,
                    last_position: event.position,
                    start_position: event.position,
                    moved: false,
                });
            }
            MouseButton::Middle => {
                self.gizmo_press_in_box = false;
                self.drag = Some(DragState {
                    kind: DragKind::Pan,
                    last_position: event.position,
                    start_position: event.position,
                    moved: false,
                });
            }
            _ => {}
        }
    }

    fn handle_mouse_move(&mut self, event: &MouseMoveEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if !self.is_gpu_ready() {
            return;
        }
        let pointer = point_to_vec2(event.position);
        let hover = self.gizmo_rect().and_then(|r| gizmo::hit_test(r, &self.viewport.camera, pointer));
        let hover_changed = hover != self.gizmo_hover;
        self.gizmo_hover = hover;

        let mut camera_changed = false;
        if let Some(drag) = self.drag {
            let dx = event.position.x.as_f32() - drag.last_position.x.as_f32();
            let dy = event.position.y.as_f32() - drag.last_position.y.as_f32();
            if dx != 0.0 || dy != 0.0 {
                let size = self.viewport_size();
                match drag.kind {
                    DragKind::Orbit => self.viewport.camera.orbit((dx, dy), size),
                    DragKind::Pan => self.viewport.camera.pan((dx, dy), size),
                }
                let total_dx = event.position.x.as_f32() - drag.start_position.x.as_f32();
                let total_dy = event.position.y.as_f32() - drag.start_position.y.as_f32();
                let moved = drag.moved || (total_dx * total_dx + total_dy * total_dy).sqrt() > DRAG_THRESHOLD_PX;
                self.drag = Some(DragState { last_position: event.position, moved, ..drag });
                camera_changed = true;
            }
        }
        if camera_changed {
            self.viewport.mark_dirty();
        }

        // Plate picking only runs while not dragging the camera/gizmo, so it
        // doesn't fight with (or waste CPU during) an orbit/pan; any hover
        // from before the drag started is cleared so the tooltip/highlight
        // don't sit stale over a moving model.
        let plate_hover_changed =
            if self.drag.is_none() { self.update_plate_hover(event.position) } else { self.clear_plate_hover() };

        if camera_changed || hover_changed || plate_hover_changed {
            cx.notify();
        }
    }

    fn handle_mouse_up(&mut self, event: &MouseUpEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let drag = self.drag.take();
        if !self.is_gpu_ready() {
            self.gizmo_press_in_box = false;
            return;
        }
        let plain_click = drag.is_some_and(|d| !d.moved);
        if event.button == MouseButton::Left && self.gizmo_press_in_box && plain_click {
            let pointer = point_to_vec2(event.position);
            if let Some(rect) = self.gizmo_rect()
                && let Some((axis, positive)) = gizmo::hit_test(rect, &self.viewport.camera, pointer)
            {
                self.snap_camera(axis, positive, cx);
            }
        } else if event.button == MouseButton::Left && !self.gizmo_press_in_box && plain_click {
            // A plain click (not a drag) on the model, outside the gizmo box:
            // toggle the hovered plate's visibility, matching the egui app's
            // `response.clicked()` click-to-hide (`tab.rs:5350-5362`).
            if let Some(key) = self.hovered.as_ref().map(|h| h.key.clone()) {
                self.toggle_plate(key, cx);
            }
        }
        self.gizmo_press_in_box = false;
    }

    fn handle_mouse_up_out(&mut self, _event: &MouseUpEvent, _window: &mut Window, cx: &mut Context<Self>) {
        self.gizmo_press_in_box = false;
        if self.drag.take().is_some() {
            cx.notify();
        }
    }

    fn handle_scroll_wheel(&mut self, event: &ScrollWheelEvent, window: &mut Window, cx: &mut Context<Self>) {
        if !self.is_gpu_ready() {
            return;
        }
        let delta = event.delta.pixel_delta(window.line_height());
        let dy = delta.y.as_f32();
        if dy != 0.0 {
            self.viewport.camera.zoom(dy);
            self.viewport.mark_dirty();
            cx.notify();
        }
    }

    /// Ctrl/Cmd+Z (no shift) undoes; Ctrl/Cmd+Shift+Z or Ctrl/Cmd+R redoes.
    /// Otherwise WASD moves the camera target and arrows move it vertically /
    /// orbit the azimuth, tracking the key as held and (re)starting the
    /// ~60Hz movement ticker (`start_key_ticker`), which applies the
    /// per-tick camera delta every tick for smooth continuous motion with no
    /// OS auto-repeat delay, for as long as any movement key remains held.
    /// Ports `armor_viewer::common::handle_undo_redo`'s key detection.
    fn handle_key_down(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if !self.is_gpu_ready() {
            return;
        }
        if !event.is_held {
            let modifiers = event.keystroke.modifiers;
            let key = event.keystroke.key.as_str();
            if modifiers.secondary() && !modifiers.shift && key == "z" {
                self.undo_visibility(cx);
                return;
            }
            if modifiers.secondary() && (key == "r" || (key == "z" && modifiers.shift)) {
                self.redo_visibility(cx);
                return;
            }
        }
        let Some(key) = MoveKey::from_str(event.keystroke.key.as_str()) else { return };
        if self.held_keys.insert(key) {
            self.start_key_ticker(cx);
        }
    }

    /// Removes a released movement key from the held set. The ticker itself
    /// notices an empty set and stops (no idle cost once nothing is held).
    fn handle_key_up(&mut self, event: &KeyUpEvent, _window: &mut Window, _cx: &mut Context<Self>) {
        if let Some(key) = MoveKey::from_str(event.keystroke.key.as_str()) {
            self.held_keys.remove(&key);
        }
    }

    /// Applies one tick's worth of camera movement for every currently held
    /// movement key, mirroring the egui original's per-frame `key_down` poll
    /// (`viewport_3d/camera.rs` `handle_input`).
    fn apply_held_keys(&mut self) -> bool {
        if self.held_keys.is_empty() {
            return false;
        }
        let mut fwd = 0.0f32;
        let mut right = 0.0f32;
        let mut vert = 0.0f32;
        let mut rot = 0.0f32;
        for key in &self.held_keys {
            match key {
                MoveKey::Forward => fwd += 1.0,
                MoveKey::Back => fwd -= 1.0,
                MoveKey::Left => right -= 1.0,
                MoveKey::Right => right += 1.0,
                MoveKey::Up => vert += 1.0,
                MoveKey::Down => vert -= 1.0,
                MoveKey::RotateLeft => rot += 1.0,
                MoveKey::RotateRight => rot -= 1.0,
            }
        }
        if fwd != 0.0 || right != 0.0 {
            self.viewport.camera.wasd(fwd, right);
        }
        if vert != 0.0 {
            self.viewport.camera.move_vertical(vert);
        }
        if rot != 0.0 {
            self.viewport.camera.rotate_horizontal(rot);
        }
        true
    }

    /// Runs a ~60Hz ticker that applies held-key camera movement each tick
    /// and marks the viewport dirty, for smooth continuous motion. Stops
    /// itself as soon as `held_keys` is empty, so there's no idle cost once
    /// all movement keys are released.
    fn start_key_ticker(&mut self, cx: &mut Context<Self>) {
        if self.key_ticking {
            return;
        }
        self.key_ticking = true;
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs_f32(1.0 / KEY_TICK_HZ)).await;
                let still_held = this.update(cx, |this, cx| {
                    let moved = this.apply_held_keys();
                    if moved {
                        this.viewport.mark_dirty();
                        cx.notify();
                    }
                    !this.held_keys.is_empty()
                });
                match still_held {
                    Ok(true) => continue,
                    _ => break,
                }
            }
            let _ = this.update(cx, |this, _cx| this.key_ticking = false);
        })
        .detach();
    }

    fn is_gpu_ready(&self) -> bool {
        matches!(self.gpu, GpuState::Ready { .. })
    }

    /// CPU-picks at `position` (screen space) and updates `self.hovered` to
    /// match, matching the egui app's own per-frame hover pick (`tab.rs:5326-5347`).
    /// Rebuilds the hover-highlight overlay mesh only when the hovered plate
    /// actually changed (an on-demand GPU upload); the cursor position is
    /// still refreshed on every call so the floating tooltip element tracks
    /// the pointer. Returns whether the caller should `cx.notify()`.
    fn update_plate_hover(&mut self, position: Point<Pixels>) -> bool {
        let Some(bounds) = self.last_bounds else { return self.clear_plate_hover() };
        if self.current_armor.is_none() {
            return self.clear_plate_hover();
        }
        let rect = view_rect_from_bounds(bounds);
        let hit = self.viewport.pick(point_to_vec2(position), rect);
        let found =
            hit.as_ref().and_then(|h| picking_ui::tooltip_for_hit(h, &self.mesh_triangle_info)).map(|tooltip| {
                let key = picking_ui::plate_key_of(tooltip);
                (key, tooltip.clone())
            });
        let Some((key, tooltip)) = found else { return self.clear_plate_hover() };

        let key_changed = self.hovered.as_ref().map(|h| h.key != key).unwrap_or(true);
        self.hovered = Some(HoverInfo { key: key.clone(), tooltip, cursor: position });
        if key_changed {
            self.rebuild_hover_highlight(&key);
        }
        true
    }

    /// Clears any hovered plate, tooltip, and highlight overlay. Returns
    /// whether anything actually changed (so callers only `cx.notify()` when
    /// needed).
    fn clear_plate_hover(&mut self) -> bool {
        let had_hover = self.hovered.take().is_some();
        if let Some((_, old_id)) = self.hover_highlight.take() {
            self.viewport.remove_mesh(old_id);
            self.viewport.mark_dirty();
        }
        had_hover
    }

    /// Replaces the hover-highlight overlay mesh with one for `key`, via
    /// `picking_ui::upload_plate_highlight`. A no-op (leaves no highlight) if
    /// the GPU device isn't ready or there is no armor loaded, which should
    /// not happen in practice since `update_plate_hover` already checked both.
    fn rebuild_hover_highlight(&mut self, key: &PlateKey) {
        if let Some((_, old_id)) = self.hover_highlight.take() {
            self.viewport.remove_mesh(old_id);
        }
        let GpuState::Ready { ctx, .. } = &self.gpu else { return };
        let Some(armor) = self.current_armor.clone() else { return };
        let visibility = VisibilityFilter { part: &self.part_visibility, plate: &self.plate_visibility };
        let mesh_id = picking_ui::upload_plate_highlight(
            &mut self.viewport,
            &ctx.device,
            &armor,
            key,
            visibility,
            self.display_settings.show_zero_mm,
        );
        self.hover_highlight = Some((key.clone(), mesh_id));
        self.viewport.mark_dirty();
    }

    /// Replaces the sidebar-hover highlight overlay mesh for `key`, via
    /// `picking_ui::upload_zone_highlight`/`upload_part_highlight`/
    /// `upload_plate_highlight`. A no-op (leaves no highlight) if the GPU
    /// device isn't ready or there is no armor loaded. Also used by
    /// `reupload_current_armor` to rebuild an already-active sidebar
    /// highlight against fresh geometry after a visibility mutation.
    fn rebuild_sidebar_highlight(&mut self, key: SidebarHighlightKey) {
        if let Some((_, old_id)) = self.sidebar_highlight.take() {
            self.viewport.remove_mesh(old_id);
        }
        let GpuState::Ready { ctx, .. } = &self.gpu else { return };
        let Some(armor) = self.current_armor.clone() else { return };
        let visibility = VisibilityFilter { part: &self.part_visibility, plate: &self.plate_visibility };
        let show_zero_mm = self.display_settings.show_zero_mm;
        let mesh_id = match &key {
            SidebarHighlightKey::Zone(zone) => picking_ui::upload_zone_highlight(
                &mut self.viewport,
                &ctx.device,
                &armor,
                zone,
                visibility,
                show_zero_mm,
            ),
            SidebarHighlightKey::Part(zone, part) => picking_ui::upload_part_highlight(
                &mut self.viewport,
                &ctx.device,
                &armor,
                zone,
                part,
                visibility,
                show_zero_mm,
            ),
            SidebarHighlightKey::Plate(pk) => picking_ui::upload_plate_highlight(
                &mut self.viewport,
                &ctx.device,
                &armor,
                pk,
                visibility,
                show_zero_mm,
            ),
        };
        self.sidebar_highlight = Some((key, mesh_id));
        self.viewport.mark_dirty();
    }

    /// Sets the visibility popover's row-hover highlight to `key`, rebuilding
    /// the overlay mesh only when the hovered key actually changed. Called
    /// from `popover.rs`'s row `on_hover(true)` handlers.
    pub(crate) fn set_sidebar_hover(&mut self, key: SidebarHighlightKey, cx: &mut Context<Self>) {
        if self.sidebar_highlight.as_ref().map(|(k, _)| k) == Some(&key) {
            return;
        }
        self.rebuild_sidebar_highlight(key);
        cx.notify();
    }

    /// Clears the sidebar-hover highlight unconditionally: the popover closed.
    pub(crate) fn clear_sidebar_hover(&mut self, cx: &mut Context<Self>) {
        if let Some((_, old_id)) = self.sidebar_highlight.take() {
            self.viewport.remove_mesh(old_id);
            self.viewport.mark_dirty();
            cx.notify();
        }
    }

    /// Clears the sidebar-hover highlight only if it currently matches `key`:
    /// a row's mouse-leave firing after a different row's mouse-enter already
    /// switched the highlight must not clobber the new one.
    pub(crate) fn clear_sidebar_hover_if(&mut self, key: &SidebarHighlightKey, cx: &mut Context<Self>) {
        if self.sidebar_highlight.as_ref().map(|(k, _)| k) == Some(key) {
            self.clear_sidebar_hover(cx);
        }
    }

    /// Snapshots `{part_visibility, plate_visibility}` onto the undo stack,
    /// runs `mutate`, then re-uploads. The single seam every visibility
    /// mutation goes through -- click-to-hide, the context menu, and every
    /// popover action alike -- so no call site can forget to record an undo
    /// step or a re-upload.
    fn snapshot_and_mutate(&mut self, cx: &mut Context<Self>, mutate: impl FnOnce(&mut Self)) {
        self.undo_stack.push(VisibilitySnapshot {
            part_visibility: self.part_visibility.clone(),
            plate_visibility: self.plate_visibility.clone(),
        });
        mutate(self);
        self.reupload_current_armor(cx);
    }

    /// Toggles a single plate's visibility. Shared by the raycast click-to-
    /// hide handler (`handle_mouse_up`), the context menu's hide/show item,
    /// and the popover's plate rows.
    pub(crate) fn toggle_plate(&mut self, key: PlateKey, cx: &mut Context<Self>) {
        self.snapshot_and_mutate(cx, |this| {
            let hidden = this.plate_visibility.get(&key).copied().unwrap_or(false);
            this.set_plate_hidden_raw(key, !hidden);
        });
    }

    /// Sets one plate's hidden state directly, with no snapshot/re-upload of
    /// its own -- callers go through `snapshot_and_mutate`. `plate_visibility`
    /// stores only explicitly-hidden keys (absent = visible), so showing a
    /// plate removes its entry rather than inserting `false`.
    fn set_plate_hidden_raw(&mut self, key: PlateKey, hidden: bool) {
        if hidden {
            self.plate_visibility.insert(key, true);
        } else {
            self.plate_visibility.remove(&key);
        }
    }

    /// Clears every explicitly-hidden plate ("Show all hidden plates" context
    /// menu action, `tab.rs:5393-5402`).
    fn show_all_plates(&mut self, cx: &mut Context<Self>) {
        if self.plate_visibility.is_empty() {
            return;
        }
        self.snapshot_and_mutate(cx, |this| this.plate_visibility.clear());
    }

    /// Sets `(zone, material)`'s part-level visibility off ("Disable
    /// {material}" context menu action, `tab.rs:5406-5414`; matches the
    /// current egui behavior, which uses `part_visibility`, not per-plate
    /// hides -- Task 6's version predated `part_visibility` and reproduced
    /// the same effect by hiding every plate thickness individually).
    fn disable_material(&mut self, zone: &str, material: &str, cx: &mut Context<Self>) {
        let key = (zone.to_string(), material.to_string());
        self.snapshot_and_mutate(cx, |this| {
            this.part_visibility.insert(key, false);
        });
    }

    /// "All" popover button: every `(zone, part)` on, all plate overrides
    /// cleared. Ports the egui `all_btn` handler (`tab.rs:4412-4424`).
    pub(crate) fn set_all_parts_visible(&mut self, cx: &mut Context<Self>) {
        let Some(armor) = self.current_armor.clone() else { return };
        self.snapshot_and_mutate(cx, |this| {
            for (zone, parts) in &armor.zone_parts {
                for part in parts {
                    this.part_visibility.insert((zone.clone(), part.clone()), true);
                }
            }
            this.plate_visibility.clear();
        });
    }

    /// "None" popover button: every `(zone, part)` off. Ports the egui
    /// `none_btn` handler (`tab.rs:4425-4436`).
    pub(crate) fn set_all_parts_hidden(&mut self, cx: &mut Context<Self>) {
        let Some(armor) = self.current_armor.clone() else { return };
        self.snapshot_and_mutate(cx, |this| {
            for (zone, parts) in &armor.zone_parts {
                for part in parts {
                    this.part_visibility.insert((zone.clone(), part.clone()), false);
                }
            }
        });
    }

    /// "Reset plates" popover button: clears plate-level overrides only,
    /// leaving `part_visibility` untouched. Ports `tab.rs:4439-4446`.
    pub(crate) fn reset_plate_overrides(&mut self, cx: &mut Context<Self>) {
        if self.plate_visibility.is_empty() {
            return;
        }
        self.snapshot_and_mutate(cx, |this| this.plate_visibility.clear());
    }

    /// Toggles one `(zone, part)`'s visibility from the popover: sets
    /// `part_visibility[(zone,part)] = checked` and unconditionally clears
    /// every one of `plate_thicknesses`' plate overrides for that part (not
    /// gated on `checked`, matching egui's own part-row handlers,
    /// `tab.rs:4529-4542` and `4559-4569`, both of which clear regardless of
    /// the new checked state -- unlike the zone-level toggle below, which
    /// only clears when turning ON).
    pub(crate) fn toggle_part(
        &mut self,
        zone: String,
        part: String,
        plate_thicknesses: Vec<i32>,
        checked: bool,
        cx: &mut Context<Self>,
    ) {
        self.snapshot_and_mutate(cx, move |this| {
            visibility::clear_plate_overrides(&mut this.plate_visibility, &zone, &part, &plate_thicknesses);
            this.part_visibility.insert((zone, part), checked);
        });
    }

    /// Toggles an entire zone's parts from the popover's zone-header checkbox
    /// (`checked`), or -- when `solo` is set (Ctrl/Cmd-click) -- solos this
    /// zone: its parts on, every other zone's parts off, and every zone's
    /// plate overrides cleared. Ports `tab.rs:4482-4511`.
    pub(crate) fn toggle_zone(&mut self, zone: String, checked: bool, solo: bool, cx: &mut Context<Self>) {
        let Some(armor) = self.current_armor.clone() else { return };
        self.snapshot_and_mutate(cx, move |this| {
            if solo {
                for z in &armor.zone_part_plates {
                    let on = z.name == zone;
                    for p in &z.parts {
                        this.part_visibility.insert((z.name.clone(), p.name.clone()), on);
                        for &t in &p.plates {
                            this.plate_visibility.remove(&(z.name.clone(), p.name.clone(), t));
                        }
                    }
                }
                return;
            }
            let Some(z) = armor.zone_part_plates.iter().find(|z| z.name == zone) else { return };
            for p in &z.parts {
                this.part_visibility.insert((zone.clone(), p.name.clone()), checked);
                if checked {
                    for &t in &p.plates {
                        this.plate_visibility.remove(&(zone.clone(), p.name.clone(), t));
                    }
                }
            }
        });
    }

    /// Toggles a zone header's expanded/collapsed state in the popover tree.
    /// Purely local UI state; not undo-tracked.
    pub(crate) fn toggle_zone_expanded(&mut self, zone: String, cx: &mut Context<Self>) {
        if !self.expanded_zones.remove(&zone) {
            self.expanded_zones.insert(zone);
        }
        cx.notify();
    }

    /// Toggles a material/part header's expanded/collapsed state in the
    /// popover tree. Purely local UI state; not undo-tracked.
    pub(crate) fn toggle_part_expanded(&mut self, key: (String, String), cx: &mut Context<Self>) {
        if !self.expanded_parts.remove(&key) {
            self.expanded_parts.insert(key);
        }
        cx.notify();
    }

    /// Toggles a hull-part-group header's expanded/collapsed state in the
    /// hull-visibility popover tree. Purely local UI state; not undo-tracked.
    pub(crate) fn toggle_hull_group_expanded(&mut self, group: String, cx: &mut Context<Self>) {
        if !self.expanded_hull_groups.remove(&group) {
            self.expanded_hull_groups.insert(group);
        }
        cx.notify();
    }

    fn current_visibility_snapshot(&self) -> VisibilitySnapshot {
        VisibilitySnapshot {
            part_visibility: self.part_visibility.clone(),
            plate_visibility: self.plate_visibility.clone(),
        }
    }

    /// Ctrl/Cmd+Z: restores the previous visibility snapshot. Ports
    /// `armor_viewer::common::handle_undo_redo`'s undo branch.
    pub(crate) fn undo_visibility(&mut self, cx: &mut Context<Self>) {
        let current = self.current_visibility_snapshot();
        let Some(prev) = self.undo_stack.undo(current) else { return };
        self.part_visibility = prev.part_visibility;
        self.plate_visibility = prev.plate_visibility;
        self.reupload_current_armor(cx);
    }

    /// Ctrl/Cmd+Shift+Z or Ctrl/Cmd+R: re-applies the next visibility
    /// snapshot. Ports `armor_viewer::common::handle_undo_redo`'s redo branch.
    pub(crate) fn redo_visibility(&mut self, cx: &mut Context<Self>) {
        let current = self.current_visibility_snapshot();
        let Some(next) = self.undo_stack.redo(current) else { return };
        self.part_visibility = next.part_visibility;
        self.plate_visibility = next.plate_visibility;
        self.reupload_current_armor(cx);
    }

    /// Re-uploads `current_armor` honoring the current `part_visibility`/
    /// `plate_visibility`, without moving the camera
    /// (`upload::reupload_armor_plates`). If the raycast-hovered plate is
    /// still visible its highlight is rebuilt against the fresh geometry; if
    /// it just became hidden the hover/tooltip are cleared outright rather
    /// than waiting for the next mouse-move. The popover's sidebar-hover
    /// highlight, if active, is likewise rebuilt against the fresh geometry.
    fn reupload_current_armor(&mut self, cx: &mut Context<Self>) {
        let GpuState::Ready { ctx, pipeline } = &self.gpu else { return };
        let Some(armor) = self.current_armor.clone() else { return };
        let device = ctx.device.clone();
        let queue = ctx.queue.clone();
        let visibility = VisibilityFilter { part: &self.part_visibility, plate: &self.plate_visibility };
        self.mesh_triangle_info =
            upload::reupload_armor_plates(&mut self.viewport, &device, &armor, visibility, self.display_settings);
        // `viewport.clear()` (inside the armor re-upload above) wipes every
        // mesh, hull included -- top the hull back up now that the armor pass
        // is done, keeping the two upload paths decoupled (a hull-only change
        // never lands here, see `reupload_hull`) while staying in sync with
        // whatever `Viewport3D::clear` just destroyed.
        upload_hull::upload_hull_meshes(
            &mut self.viewport,
            &device,
            &queue,
            pipeline,
            &armor,
            &mut self.hull_mesh_ids,
            &self.hull_visibility,
            self.display_settings.hull_opaque,
            &self.active_camo_textures,
            &self.active_camo_uvs,
        );
        // `viewport.clear()` (inside the re-upload) already dropped the old
        // highlight meshes; forget the stale ids before rebuilding against
        // the fresh geometry.
        self.hover_highlight = None;
        if let Some(hover) = self.hovered.clone() {
            let still_visible = self
                .mesh_triangle_info
                .iter()
                .flat_map(|(_, tooltips)| tooltips)
                .any(|t| picking_ui::plate_key_of(t) == hover.key);
            if still_visible {
                self.rebuild_hover_highlight(&hover.key);
            } else {
                self.hovered = None;
            }
        }
        if let Some((key, _)) = self.sidebar_highlight.take() {
            self.rebuild_sidebar_highlight(key);
        }
        self.viewport.mark_dirty();
        cx.notify();
    }

    /// Mutates `display_settings` via `mutate` and re-uploads the armor --
    /// every display-settings popover checkbox and slider (`popover.rs`)
    /// goes through this single seam, so no call site can forget the
    /// re-upload these settings need (they affect geometry: edge outlines,
    /// the water plane, per-vertex alpha, and 0mm filtering).
    pub(crate) fn mutate_display_settings(
        &mut self,
        cx: &mut Context<Self>,
        mutate: impl FnOnce(&mut upload::DisplaySettings),
    ) {
        mutate(&mut self.display_settings);
        self.reupload_current_armor(cx);
    }

    /// Re-uploads only the hull meshes (`upload_hull::upload_hull_meshes`),
    /// honoring the current `hull_visibility`/`display_settings.hull_opaque`.
    /// Never touches armor meshes, visibility overrides, or the camera, and
    /// never calls `Viewport3D::clear()` itself -- a hull-only change stays
    /// cheap even on a ship with a large armor mesh set. A no-op if the GPU
    /// device isn't ready or there is no armor loaded.
    fn reupload_hull(&mut self) {
        let GpuState::Ready { ctx, pipeline } = &self.gpu else { return };
        let Some(armor) = self.current_armor.clone() else { return };
        let device = ctx.device.clone();
        let queue = ctx.queue.clone();
        upload_hull::upload_hull_meshes(
            &mut self.viewport,
            &device,
            &queue,
            pipeline,
            &armor,
            &mut self.hull_mesh_ids,
            &self.hull_visibility,
            self.display_settings.hull_opaque,
            &self.active_camo_textures,
            &self.active_camo_uvs,
        );
    }

    /// Mutates `hull_visibility` via `mutate` and re-uploads only the hull --
    /// the hull-visibility popover's All/None buttons and per-mesh checkboxes
    /// (`popover.rs`) all go through this single seam. Unlike
    /// `mutate_display_settings`, this never touches armor meshes or the
    /// camera (see `reupload_hull`'s doc).
    pub(crate) fn mutate_hull_visibility(
        &mut self,
        cx: &mut Context<Self>,
        mutate: impl FnOnce(&mut HashMap<String, bool>),
    ) {
        mutate(&mut self.hull_visibility);
        self.reupload_hull();
        cx.notify();
    }

    /// Sets `display_settings.hull_opaque` and re-uploads only the hull (its
    /// alpha and depth-write layer both depend on this flag, see
    /// `upload_hull::upload_hull_meshes`'s doc). Kept separate from
    /// `mutate_display_settings` so toggling hull opacity -- a hull-only
    /// change -- never rebuilds the armor mesh set.
    pub(crate) fn set_hull_opaque(&mut self, opaque: bool, cx: &mut Context<Self>) {
        if self.display_settings.hull_opaque == opaque {
            return;
        }
        self.display_settings.hull_opaque = opaque;
        self.reupload_hull();
        cx.notify();
    }

    /// Selects (or clears, `id: None`) the active camo scheme and re-uploads
    /// only the hull. Ports the egui app's camo-change flow (`tab.rs:797-847`):
    /// decode `id`'s textures (cache hit skips the decode), look up its
    /// `CamoSchemeInfo` for `uv_transforms`/`use_color_scheme`, composite them
    /// against `armor.hull_textures` via `camo::build_active_camo`, and store
    /// the result as `active_camo_textures`/`active_camo_uvs`. A decode
    /// failure, or a selected id with no matching `camo_scheme_infos` entry,
    /// is logged via `tracing::warn!` and treated as stock (empty active-camo
    /// maps) rather than left half-applied. The decode/composite step itself
    /// is [`recompute_active_camo`] -- also called by `apply_reload_result`
    /// to re-apply the same `selected_camo` against a freshly reloaded
    /// armor's hull textures (a hull/LOD/module reload).
    pub(crate) fn select_camo(&mut self, id: Option<CamoSchemeId>, cx: &mut Context<Self>) {
        self.selected_camo = id;
        match self.current_armor.clone() {
            Some(armor) => {
                let (t, u) = recompute_active_camo(self.selected_camo, &mut self.camo_texture_cache, &armor);
                self.active_camo_textures = t;
                self.active_camo_uvs = u;
            }
            None => {
                self.active_camo_textures.clear();
                self.active_camo_uvs.clear();
            }
        }
        self.reupload_hull();
        cx.notify();
    }

    /// Hull-upgrade selector (`popover.rs`): switches to `key`'s hull upgrade,
    /// clears `selected_modules` (its alternatives may differ per hull,
    /// matching the egui app's own selector, `tab.rs:3169`), and reloads. A
    /// no-op if `key` is already selected.
    pub(crate) fn select_hull_upgrade(&mut self, key: String, cx: &mut Context<Self>) {
        if self.selected_hull.as_ref() == Some(&key) {
            return;
        }
        self.selected_hull = Some(key);
        self.selected_modules.clear();
        self.reload_ship(cx);
    }

    /// LOD selector (`popover.rs`): switches to `lod` and reloads. A no-op if
    /// `lod` is already selected.
    pub(crate) fn select_hull_lod(&mut self, lod: usize, cx: &mut Context<Self>) {
        if self.hull_lod == lod {
            return;
        }
        self.hull_lod = lod;
        self.reload_ship(cx);
    }

    /// Module-alternative selector (`popover.rs`): overrides `ct`'s component
    /// to `name` and reloads. A no-op if `name` is already selected for `ct`.
    pub(crate) fn select_module_alternative(&mut self, ct: ComponentType, name: String, cx: &mut Context<Self>) {
        if self.selected_modules.get(&ct) == Some(&name) {
            return;
        }
        self.selected_modules.insert(ct, name);
        self.reload_ship(cx);
    }

    /// Kicks off a background full reload of the current ship under
    /// `selected_hull`/`hull_lod`/`selected_modules`, driven by the hull
    /// popover's selectors. A no-op if no ship has ever been loaded
    /// (`reload_source` unset) -- should not happen in practice, since the
    /// selectors that call this only render once `current_armor` is `Some`.
    ///
    /// **Full reload, not egui's incremental path.** The egui app has two
    /// narrower reloads -- hull-only (`start_hull_lod_reload`) and
    /// upgrade-only (`start_upgrade_reload`) -- that mutate an existing
    /// `LoadedShipArmor` in place (`tab.rs:2472`, `2568`). This port's
    /// `LoadedShipArmor` is an immutable `Arc` (never mutated once loaded,
    /// see `load_ship.rs`'s doc), so a full re-export via
    /// [`spawn_reload_ship_armor`] is used for all three selectors instead --
    /// correct for all of them (a hull-upgrade change also swaps turret
    /// armor, which a full reload naturally handles), at the cost of
    /// re-exporting armor geometry on a LOD-only change too (a perf, not
    /// correctness, deviation).
    pub(crate) fn reload_ship(&mut self, cx: &mut Context<Self>) {
        let Some(source) = &self.reload_source else { return };
        let bundle = Arc::clone(&source.bundle);
        let param_index = source.param_index.clone();
        let display_name = source.display_name.clone();

        let options = match load_ship::build_reload_options(
            &bundle.assets,
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

        self.reload_generation += 1;
        let generation = self.reload_generation;
        let task = spawn_reload_ship_armor(bundle, param_index, options, cx);
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| this.apply_reload_result(generation, result, cx));
        })
        .detach();
    }

    /// Applies a completed `reload_ship` result. Discards it if a newer
    /// reload superseded this one (`generation` guard, mirroring
    /// `ArmorViewerPane::apply_ship_load_result`'s identical pattern for ship
    /// selection). Otherwise: retains whatever `hull_visibility`/
    /// `part_visibility`/`plate_visibility` entries still apply to the new
    /// armor (`visibility::retain_*`; a hull/LOD/module change can add or
    /// drop parts/plates/hull meshes), re-applies the active camo against the
    /// new armor's hull textures (`recompute_active_camo`), replaces
    /// `current_armor`, and re-uploads armor + hull WITHOUT reframing the
    /// camera (`upload::reupload_armor_plates`, not `upload_armor_to_viewport`)
    /// -- the user is mid-view when they change a hull/LOD/module selector,
    /// and a reload must not jump it. Clears the undo/redo history and any
    /// hover/sidebar highlight, since either can reference a part/plate/mesh
    /// the reload just removed, matching the egui app's own
    /// `apply_upgrade_reload` (`tab.rs:2737`).
    fn apply_reload_result(
        &mut self,
        generation: u64,
        result: Result<LoadedShipArmor, ShipLoadError>,
        cx: &mut Context<Self>,
    ) {
        if generation != self.reload_generation {
            return;
        }
        let armor = match result {
            Ok(armor) => Arc::new(armor),
            Err(e) => {
                tracing::error!("armor viewer: failed to reload ship armor: {e}");
                return;
            }
        };
        let GpuState::Ready { ctx, pipeline } = &self.gpu else { return };
        let device = ctx.device.clone();
        let queue = ctx.queue.clone();

        visibility::retain_hull_visibility(&mut self.hull_visibility, &armor.hull_part_groups);
        visibility::retain_part_visibility(&mut self.part_visibility, &armor.zone_parts);
        visibility::retain_plate_visibility(&mut self.plate_visibility, &armor.zone_part_plates);
        self.undo_stack.clear();
        self.hovered = None;
        self.hover_highlight = None;
        self.sidebar_highlight = None;

        let (active_camo_textures, active_camo_uvs) =
            recompute_active_camo(self.selected_camo, &mut self.camo_texture_cache, &armor);
        self.active_camo_textures = active_camo_textures;
        self.active_camo_uvs = active_camo_uvs;
        self.model_bounds = Some(armor.bounds);

        let visibility_filter = VisibilityFilter { part: &self.part_visibility, plate: &self.plate_visibility };
        self.mesh_triangle_info = upload::reupload_armor_plates(
            &mut self.viewport,
            &device,
            &armor,
            visibility_filter,
            self.display_settings,
        );
        upload_hull::upload_hull_meshes(
            &mut self.viewport,
            &device,
            &queue,
            pipeline,
            &armor,
            &mut self.hull_mesh_ids,
            &self.hull_visibility,
            self.display_settings.hull_opaque,
            &self.active_camo_textures,
            &self.active_camo_uvs,
        );
        self.viewport.mark_dirty();
        self.current_armor = Some(armor);
        cx.notify();
    }

    /// Toggles a camo-origin group header's expanded/collapsed state in the
    /// hull popover's camo picker. Purely local UI state; not undo-tracked.
    pub(crate) fn toggle_camo_group_expanded(&mut self, group: String, cx: &mut Context<Self>) {
        if !self.expanded_camo_groups.remove(&group) {
            self.expanded_camo_groups.insert(group);
        }
        cx.notify();
    }

    /// Current lighting settings, for `popover.rs` to read while building the
    /// display-settings popover's Hull Lighting section (`viewport` itself is
    /// private -- this is the read-only seam, `mutate_lighting`/
    /// `set_lighting_preset` below are the write seam).
    pub(crate) fn lighting(&self) -> LightingSettings {
        self.viewport.lighting.clone()
    }

    /// Mutates the viewport's lighting via `mutate` and marks it dirty for a
    /// redraw. Lighting is a per-frame uniform (`Viewport3D::lighting`), so
    /// unlike `mutate_display_settings` this never re-uploads a mesh.
    pub(crate) fn mutate_lighting(&mut self, cx: &mut Context<Self>, mutate: impl FnOnce(&mut LightingSettings)) {
        mutate(&mut self.viewport.lighting);
        self.viewport.mark_dirty();
        cx.notify();
    }

    /// Applies a lighting preset (In-game/Flat/Studio popover buttons):
    /// replaces `viewport.lighting` wholesale but keeps `enabled` across the
    /// swap, matching the egui original's own preset handlers (`tab.rs:5055-
    /// 5075`, each of which reads `pane.lighting.enabled` before overwriting
    /// `pane.lighting` and restores it after). A preset moves every lighting
    /// slider's thumb at once, so the sliders are rebuilt (not `set_value`d
    /// -- see `LightingSliders`'s doc) rather than just the domain value.
    pub(crate) fn set_lighting_preset(&mut self, mut preset: LightingSettings, cx: &mut Context<Self>) {
        preset.enabled = self.viewport.lighting.enabled;
        self.viewport.lighting = preset.clone();
        let (lighting_sliders, subs) = Self::build_lighting_sliders(cx, &preset);
        self.lighting_sliders = lighting_sliders;
        self._lighting_slider_subscriptions = subs;
        self.viewport.mark_dirty();
        cx.notify();
    }

    fn snap_camera(&mut self, axis: Axis, positive: bool, cx: &mut Context<Self>) {
        let (az, el) = camera::ortho_view(axis, positive, self.viewport.camera.azimuth);
        self.viewport.camera.animate_to(az, el, GIZMO_SNAP_DURATION_SECS);
        self.viewport.mark_dirty();
        cx.notify();
        self.start_animation_ticker(cx);
    }

    /// Advances the camera's snap animation on a short-lived ~60Hz ticker,
    /// bounded to the animation's own duration (`update_animation` returns
    /// false once it finishes, which stops the loop). This is the one
    /// deliberate exception to on-demand rendering: while the user-triggered
    /// transition is in flight, each tick marks the viewport dirty so the
    /// eased motion is visible; once it settles, redraws go back to being
    /// purely input-driven.
    fn start_animation_ticker(&mut self, cx: &mut Context<Self>) {
        if self.animating {
            return;
        }
        self.animating = true;
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_millis(16)).await;
                let still_animating = this.update(cx, |this, cx| {
                    let still = this.viewport.camera.update_animation(1.0 / 60.0);
                    this.viewport.mark_dirty();
                    cx.notify();
                    still
                });
                match still_animating {
                    Ok(true) => continue,
                    _ => break,
                }
            }
            let _ = this.update(cx, |this, _cx| this.animating = false);
        })
        .detach();
    }
}

impl Render for ViewportView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.redraw_if_needed(window);

        let status: Option<String> = match &self.gpu {
            GpuState::Initializing => Some("Initializing 3D viewport...".to_string()),
            GpuState::Failed(reason) => Some(format!("Armor viewport failed to initialize: {reason}")),
            GpuState::Ready { .. } => None,
        };
        let image_child = match self.image.clone() {
            Some(image) => img(image).object_fit(ObjectFit::Fill).size_full().into_any_element(),
            None => h_flex()
                .size_full()
                .items_center()
                .justify_center()
                .child(div().text_sm().opacity(0.6).child(status.unwrap_or_else(|| "Rendering...".to_string())))
                .into_any_element(),
        };

        let camera_snapshot = self.viewport.camera.clone();
        let hover = self.gizmo_hover;
        let weak = cx.weak_entity();
        let overlay = canvas(
            move |bounds: Bounds<Pixels>, _window, cx| {
                let _ = weak.update(cx, |this, cx| {
                    if this.last_bounds != Some(bounds) {
                        this.last_bounds = Some(bounds);
                        this.viewport.mark_dirty();
                        cx.notify();
                    }
                });
                bounds
            },
            move |bounds, bounds_again, window, cx| {
                draw_gizmo_overlay(bounds_again, &camera_snapshot, hover, window, cx);
                let _ = bounds;
            },
        )
        .absolute()
        .size_full();

        // Floating thickness tooltip, anchored near (not under) the cursor
        // and snapped back inside the window if it would overflow -- reuses
        // gpui's own `anchored()`/`deferred()` primitives (the same ones the
        // context menu below and gpui-component's managed tooltips use)
        // rather than hand-measuring the tooltip's size.
        let tooltip_overlay = self.hovered.as_ref().map(|hover| {
            let theme = cx.theme();
            let (background, border, radius, muted) =
                (theme.background, theme.border, theme.radius, theme.muted_foreground);
            let element = picking_ui::tooltip_element(&hover.tooltip, background, border, radius, muted);
            let anchor_pos = point(hover.cursor.x + TOOLTIP_CURSOR_OFFSET, hover.cursor.y + TOOLTIP_CURSOR_OFFSET);
            deferred(anchored().position(anchor_pos).snap_to_window_with_margin(px(8.)).child(element))
                .with_priority(0)
                .into_any_element()
        });

        // Right-click context menu: hide/show the hovered plate, show all
        // hidden plates, and disable the hovered part's material. Snapshots
        // `self.hovered`/`plate_visibility` at render time (refreshed on
        // every hover-driven `cx.notify()`, so effectively current by the
        // time a right-click follows a real hover) since the menu-item click
        // handlers only get `&mut App`, not this view's own `Context`.
        let entity = cx.entity();
        let hovered_for_menu = self.hovered.clone();
        let hover_is_hidden =
            hovered_for_menu.as_ref().is_some_and(|h| self.plate_visibility.get(&h.key).copied().unwrap_or(false));
        let hidden_count = self.plate_visibility.len();

        // Toolbar row above the viewport: the armor-visibility popover
        // trigger today, with room for Task 7b's display-settings button.
        let toolbar = popover::render_toolbar(&*self, &entity, cx);

        let viewport_area = div()
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
            .on_scroll_wheel(cx.listener(Self::handle_scroll_wheel))
            .on_key_down(cx.listener(Self::handle_key_down))
            .on_key_up(cx.listener(Self::handle_key_up))
            .child(image_child)
            .child(overlay)
            .when_some(tooltip_overlay, |this, t| this.child(t))
            .context_menu(move |mut menu, _window, _cx| {
                let Some(hover) = hovered_for_menu.clone() else { return menu };
                let (zone, material, thickness_tenths) = hover.key.clone();
                let thickness_mm = thickness_tenths as f32 / 10.0;

                let toggle_key = hover.key.clone();
                let toggle_entity = entity.clone();
                let toggle_label = if hover_is_hidden {
                    format!("Show {thickness_mm:.0} mm {material}")
                } else {
                    format!("Hide {thickness_mm:.0} mm {material}")
                };
                menu = menu.item(PopupMenuItem::new(toggle_label).on_click(move |_event, _window, cx| {
                    let key = toggle_key.clone();
                    toggle_entity.update(cx, |view, cx| view.toggle_plate(key, cx));
                }));

                if hidden_count > 0 {
                    let show_all_entity = entity.clone();
                    menu = menu.item(PopupMenuItem::new(format!("Show all hidden plates ({hidden_count})")).on_click(
                        move |_event, _window, cx| {
                            show_all_entity.update(cx, |view, cx| view.show_all_plates(cx));
                        },
                    ));
                }

                menu = menu.separator();

                let disable_entity = entity.clone();
                let disable_zone = zone.clone();
                let disable_material_name = material.clone();
                menu = menu.item(PopupMenuItem::new(format!("Disable {material}")).on_click(
                    move |_event, _window, cx| {
                        disable_entity
                            .update(cx, |view, cx| view.disable_material(&disable_zone, &disable_material_name, cx));
                    },
                ));

                menu
            });

        v_flex().size_full().child(toolbar).child(div().flex_1().min_h(px(0.)).child(viewport_area))
    }
}

/// Decodes and composites `selected_camo`'s textures (if any) against
/// `armor`'s hull textures, returning the active-camo texture/UV maps for
/// [`upload_hull::upload_hull_meshes`] (empty maps mean "render base albedo
/// only", matching stock). A free function (not a `&mut Self` method) so it
/// can run while a caller already holds a `&self.gpu` borrow (`ViewportView`
/// methods use `let GpuState::Ready { ctx, pipeline } = &self.gpu else {
/// return };` at the top of every re-upload path) without a whole-self
/// mutable-borrow conflict; only `camo_texture_cache` needs `&mut`, passed in
/// directly rather than through `&mut self`. A decode failure, or a selected
/// id with no matching `camo_scheme_infos` entry, is logged via
/// `tracing::warn!` and treated as stock rather than left half-applied.
#[allow(clippy::type_complexity)]
fn recompute_active_camo(
    selected_camo: Option<CamoSchemeId>,
    camo_texture_cache: &mut HashMap<CamoSchemeId, SchemeTextures>,
    armor: &LoadedShipArmor,
) -> (HashMap<String, (u32, u32, Vec<u8>)>, HashMap<String, UvTransform>) {
    let Some(id) = selected_camo else { return (HashMap::new(), HashMap::new()) };
    let decoded = match camo_texture_cache.get(&id) {
        Some(t) => Some(t.clone()),
        None => match armor.camo_source.decode(id) {
            Ok(t) => {
                if t.is_empty() {
                    tracing::warn!("camo scheme {id:?} decoded to zero textures for this ship; rendering as stock");
                }
                camo_texture_cache.insert(id, t.clone());
                Some(t)
            }
            Err(e) => {
                tracing::warn!("failed to decode camo scheme {id:?}: {e}");
                None
            }
        },
    };
    let Some(textures) = decoded else { return (HashMap::new(), HashMap::new()) };
    let info = armor.camo_scheme_infos.iter().find(|i| i.id == id);
    let (uv, use_color_scheme) = match info {
        Some(i) => (i.uv_transforms.clone(), i.use_color_scheme),
        None => {
            tracing::warn!(
                "camo scheme {id:?} decoded successfully but has no matching entry in camo_scheme_infos; falling back to identity UVs and no color scheme"
            );
            (HashMap::default(), false)
        }
    };
    build_active_camo(&textures, &uv, use_color_scheme, &armor.hull_textures)
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

fn axis_color(axis: Axis) -> Hsla {
    match axis {
        Axis::X => rgb(0xDC4646).into(),
        Axis::Y => rgb(0x5AC85A).into(),
        Axis::Z => rgb(0x5082E6).into(),
    }
}

fn axis_label(axis: Axis) -> &'static str {
    match axis {
        Axis::X => "X",
        Axis::Y => "Y",
        Axis::Z => "Z",
    }
}

fn ball_bounds(center: Point<Pixels>, radius: f32) -> Bounds<Pixels> {
    let d = px(radius * 2.0);
    Bounds::new(point(center.x - px(radius), center.y - px(radius)), size(d, d))
}

/// Draws the six axis balls (line to tip + filled/stroked circle + X/Y/Z
/// label on the positive balls), depth-ordered so the ball nearest the
/// camera draws on top, plus a white hover ring. Reuses the copied gizmo
/// projection math (`gizmo::ball_draw_order`/`gizmo_rect`) verbatim; only the
/// drawing itself is gpui-native (the egui version used `egui::Painter`).
fn draw_gizmo_overlay(
    bounds: Bounds<Pixels>,
    camera: &ArcballCamera,
    hover: Option<(Axis, bool)>,
    window: &mut Window,
    cx: &mut App,
) {
    let container = view_rect_from_bounds(bounds);
    let rect = gizmo::gizmo_rect(container);
    let center = rect.center();
    let view = camera.view_matrix();

    for (axis, positive, dir, _depth) in gizmo::ball_draw_order(&view) {
        let tip = center + dir * gizmo::ARM_LEN;
        let tip_point = vec2_to_point(tip);
        let color = axis_color(axis);

        if positive {
            let mut path = PathBuilder::stroke(px(2.0));
            path.move_to(vec2_to_point(center));
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
            paint_axis_label(axis_label(axis), tip_point, window, cx);
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

        if hover == Some((axis, positive)) {
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
    use super::Axis;
    use super::axis_color;
    use super::ball_bounds;
    use super::point_to_vec2;
    use super::vec2_to_point;
    use super::view_rect_from_bounds;
    use gpui::Bounds;
    use gpui::point;
    use gpui::px;
    use gpui::size;

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
    fn axis_colors_are_distinct() {
        let colors = [axis_color(Axis::X), axis_color(Axis::Y), axis_color(Axis::Z)];
        assert_ne!(colors[0], colors[1]);
        assert_ne!(colors[1], colors[2]);
        assert_ne!(colors[0], colors[2]);
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
