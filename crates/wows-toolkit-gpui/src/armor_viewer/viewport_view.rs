//! Armor viewport view: the 3D scene drawn into a GPUI dynamic texture, with
//! gpui_kit mouse/scroll/key input driving the copied `viewport_3d` arcball
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
//! mutation or mesh change) gates the GPU draw, done during paint into a
//! dynamic texture on GPUI's own device that GPUI then samples like an image,
//! so the gizmo, tooltips and popovers composite over it in the usual paint
//! order. Hovering the gizmo only repaints the cheap CPU overlay (no GPU
//! work). Every viewport of a pane shares one `SharedSceneDevice`; when GPUI
//! retires its device, the meshes and target made on it are released at once,
//! and the viewport uploads again on the new one when it renders (`sync_gpu`).

use std::collections::HashMap;
use std::collections::HashSet;
use std::rc::Rc;
use std::rc::Weak;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use crate::slider_field::SliderField;
use crate::slider_field::SliderFieldChange;
use crate::slider_field::SliderRange;
use gpui_kit::component::ActiveTheme;
use gpui_kit::component::button::Button;
use gpui_kit::component::h_flex;
use gpui_kit::component::menu::ContextMenuExt;
use gpui_kit::component::menu::PopupMenuItem;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::v_flex;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use gpui_wgpu::wgpu;
use rust_i18n::t;
use wows_toolkit_config::queries::ArmorViewerDefaultsRow;
use wowsunpack::export::camo_textures::CamoSchemeId;
use wowsunpack::export::camo_textures::CamoSchemeInfo;
use wowsunpack::export::camo_textures::SchemeTextures;
use wowsunpack::export::camouflage::UvTransform;
use wowsunpack::game_params::keys::ComponentType;

use super::analysis::SimAgreement;
use super::camera_rings;
use super::depth_cues;
use super::gaps;
use super::length_marks::LengthMarks;
use super::length_marks::MarkLabels;
use super::length_marks::Reading;
use super::splash;
use super::trajectory;
use super::trajectory::ArcColor;
use super::trajectory::ArcId;
use crate::armor_viewer::assets::ArmorAssetsBundle;
use crate::armor_viewer::assets::ModelAssets;
use crate::armor_viewer::camo::build_active_camo;
use crate::armor_viewer::hardpoint_overlay;
use crate::armor_viewer::hardpoint_overlay::HardpointMarkers;
use crate::armor_viewer::hull_picking;
use crate::armor_viewer::hull_picking::HighlightChange;
use crate::armor_viewer::hull_picking::HoverTarget;
use crate::armor_viewer::hull_picking::HullMeshIndex;
use crate::armor_viewer::hull_picking::MeshHit;
use crate::armor_viewer::hull_picking::MeshTooltip;
use crate::armor_viewer::legend::swatch_color;
use crate::armor_viewer::load_model;
use crate::armor_viewer::load_model::ModelLoadRequest;
use crate::armor_viewer::load_model::MountRef;
use crate::armor_viewer::load_model::ProblemReason;
use crate::armor_viewer::load_model::SceneLoadError;
use crate::armor_viewer::load_model::SceneProblem;
use crate::armor_viewer::load_ship;
use crate::armor_viewer::load_ship::ArmorTriangleTooltip;
use crate::armor_viewer::load_ship::LoadedShipArmor;
use crate::armor_viewer::load_ship::PlateKey;
use crate::armor_viewer::load_ship::SceneSubject;
use crate::armor_viewer::load_ship::ShipLoadError;
use crate::armor_viewer::load_ship::spawn_reload_ship_armor;
use crate::armor_viewer::model_refs::HardpointTarget;
use crate::armor_viewer::model_refs::OpenTarget;
use crate::armor_viewer::pane::ArmorViewerPane;
use crate::armor_viewer::picking_ui;
use crate::armor_viewer::popover;
use crate::armor_viewer::scene_load::FirstStage;
use crate::armor_viewer::scene_load::LoadCancel;
use crate::armor_viewer::scene_load::LoadSlot;
use crate::armor_viewer::scene_load::RestJob;
use crate::armor_viewer::scene_load::RestJobError;
use crate::armor_viewer::scene_load::RestPiece;
use crate::armor_viewer::scene_load::SharedLoad;
use crate::armor_viewer::upload;
use crate::armor_viewer::upload::upload_armor_to_viewport;
use crate::armor_viewer::upload_hull;
use crate::armor_viewer::upload_hull::HullTextureCache;
use crate::armor_viewer::visibility;
use crate::armor_viewer::visibility::SidebarHighlightKey;
use crate::armor_viewer::visibility::VisibilityFilter;
use crate::armor_viewer::visibility::VisibilitySnapshot;
use crate::armor_viewer::visibility::VisibilityUndoStack;
use wows_toolkit_viewmodel::armor::camera_perspective::CameraPerspective;
use wows_toolkit_viewmodel::armor::camera_perspective::LookMode;
use wows_toolkit_viewmodel::armor::camera_perspective::water_aim_point;

use wows_toolkit_viewmodel::armor::arc::ArcOutcome;
use wows_toolkit_viewmodel::armor::arc::ArcPlate;
use wows_toolkit_viewmodel::armor::arc::PlateKeyParts;
use wows_toolkit_viewmodel::armor::arc::ZoneShape;
use wows_toolkit_viewmodel::armor::arc::describe_arc;
use wows_toolkit_viewmodel::armor::arc::isolate;

use crate::viewport::camera;
use crate::viewport::camera::ArcballCamera;
use crate::viewport::camera::Axis;
use crate::viewport::device::SceneDevice;
use crate::viewport::device::SharedSceneDevice;
use crate::viewport::gizmo;
use crate::viewport::renderer::LAYER_DEFAULT;
use crate::viewport::renderer::LAYER_OVERLAY;
use crate::viewport::renderer::Viewport3D;
use crate::viewport::types::LightingSettings;
use crate::viewport::types::MeshId;
use crate::viewport::types::TargetSize;
use crate::viewport::types::Vec2;
use crate::viewport::types::Vec3;
use crate::viewport::types::ViewRect;
use wows_replay_insights::timeline::PreExtractedHit;
use wows_replays::analyzer::battle_controller::state::ResolvedShotHit;
use wows_replays::types::GameParamId;
use wows_replays::types::ShotId;
use wows_toolkit_viewmodel::armor::arc::ComparisonVerdict;
use wows_toolkit_viewmodel::armor::arc::Strike;
use wows_toolkit_viewmodel::armor::arc::compare_with_server;
use wows_toolkit_viewmodel::armor::incoming::ServerOutcome;
use wows_toolkit_viewmodel::armor::penetration::ComparisonShip;
use wowsunpack::ballistics::ImpactResult;
use wowsunpack::ballistics::ShellParams;
use wowsunpack::ballistics::solve_for_range;
use wowsunpack::game_params::ttx::armor_material::ArmorMaterial;
use wowsunpack::game_params::types::AmmoType;
use wowsunpack::game_params::types::Km;
use wowsunpack::game_params::types::Millimeters;
use wowsunpack::game_params::types::ShellInfo;
use wowsunpack::game_params::types::Species;

/// The mode a ship opens on: its first, which is the one the game itself
/// uses. `None` for a ship whose GameParams name none, where there is
/// nothing to draw.
fn first_camera_mode(armor: &LoadedShipArmor) -> Option<String> {
    armor.camera_trajectories.first().map(|(name, _)| name.clone())
}

/// How the camera orbits are drawn.
///
/// `mode` is `None` until a ship names one, and is corrected whenever the
/// loaded ship changes: a mode name belongs to a ship's own GameParams, so
/// carrying one across ships would silently draw nothing.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct CameraRingSettings {
    pub(crate) shown: bool,
    pub(crate) mode: Option<String>,
    /// Where between the field-of-view extremes the camera sits, 0 to 1.
    pub(crate) fov: f32,
    /// How far the camera is raised, -1 to 1.
    pub(crate) height: f32,
    pub(crate) zoom_path: bool,
    pub(crate) zoom_path_at_fov: bool,
    pub(crate) zoom_path_at_max_fov: bool,
}

/// The camera locked onto one of the ship's own orbits, looking the way the
/// game's camera looks from there.
///
/// What the reader steers changes with the lock: a drag turns the eye on its
/// orbit and tilts it rather than orbiting the model, and the wheel moves
/// between the inner and outer orbit rather than pulling the camera back.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct PerspectiveSettings {
    pub(crate) enabled: bool,
    pub(crate) camera: CameraPerspective,
}

/// How far the locked camera's aim marker reaches, as a fraction of the model's
/// diagonal, and the span it is held to so it is legible on any ship.
const AIM_MARKER_FRACTION: f32 = 0.03;
const AIM_MARKER_MIN: f32 = 1.0;
const AIM_MARKER_MAX: f32 = 8.0;

/// What the locked camera's aim marker is coloured (`armor_viewer/ui/tab.rs`).
const AIM_MARKER_COLOR: [f32; 4] = [1.0, 0.6, 0.1, 0.85];

/// How far along the look direction the water is searched for, so a camera
/// aimed near the horizon marks a far point rather than none.
const AIM_MARKER_REACH: f32 = 5000.0;

/// What a drag and the wheel do to the locked camera, per pixel and per notch.
const PERSPECTIVE_YAW_PER_PX: f32 = 0.005;
const PERSPECTIVE_PITCH_PER_PX: f32 = 0.005;
const PERSPECTIVE_ZOOM_PER_NOTCH: f32 = 0.001;

/// Which of the two isolations an arc is being asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Isolate {
    /// Only the plates the shell crossed.
    Plates,
    /// Every part of the zones it entered.
    Zones,
}

/// One cast arc, as the trajectory section shows it.
pub(crate) struct ArcSummary {
    pub(crate) id: ArcId,
    /// The colour the arc is drawn in, so the marker and the line match.
    pub(crate) color: ArcColor,
    pub(crate) hits: usize,
    /// The plating the shell crossed, added up regardless of angle.
    pub(crate) total_armor: Millimeters,
    pub(crate) range: Km,
    /// What it was fired with. `None` for an arc cast before any ship was
    /// chosen, which shows the plates crossed without a shell.
    pub(crate) shell: Option<CastShell>,
    pub(crate) isolating_plates: bool,
    pub(crate) isolating_zones: bool,
    pub(crate) outcome: ArcOutcome,
}

/// The ship and shell an arc is fired with.
#[derive(Clone)]
pub(crate) struct CastShell {
    pub(crate) ship: ComparisonShip,
    /// The shell in the ship's list. `None` fires its armour-piercing shell.
    pub(crate) shell_index: Option<usize>,
}

impl CastShell {
    /// Where the fired shell sits in the ship's list: the one chosen, else
    /// the ship's armour-piercing shell, which is the one a trajectory
    /// through plating says anything about, else whatever it carries rather
    /// than refusing.
    pub(crate) fn fired_index(&self) -> Option<usize> {
        let shells = &self.ship.shells;
        self.shell_index
            .filter(|index| *index < shells.len())
            .or_else(|| shells.iter().position(|shell| shell.ammo_type == AmmoType::AP))
            .or_else(|| (!shells.is_empty()).then_some(0))
    }

    /// The shell fired.
    pub(crate) fn shell(&self) -> Option<&ShellInfo> {
        self.ship.shells.get(self.fired_index()?)
    }

    /// The shell's ballistics and how it lands from `range`. `None` for a
    /// shell the solver cannot fire.
    fn fire(&self, range: Km) -> Option<(ShellParams, ImpactResult)> {
        let params = ShellParams::from_shell_info(self.shell()?)?;
        let impact = solve_for_range(&params, range.to_meters())?;
        Some((params, impact))
    }

    fn is_same(&self, other: &CastShell) -> bool {
        self.ship.param_index == other.ship.param_index && self.shell_index == other.shell_index
    }
}

/// A cast arc and what it was fired with.
struct CastArc {
    id: ArcId,
    color: ArcColor,
    shell: Option<CastShell>,
    /// The bearing the shell came in on, which a new fall angle is built on.
    approach: Vec3,
    trajectory: trajectory::Trajectory,
    /// The lines and markers `trajectory` is drawn with, worked out when it
    /// changes rather than on every frame.
    drawing: trajectory::ArcDrawing,
}

impl CastArc {
    fn set_trajectory(&mut self, trajectory: trajectory::Trajectory) {
        self.drawing = trajectory::drawing(&trajectory);
        self.trajectory = trajectory;
    }
}

/// The arc the trajectory section is editing, with the slider its range is
/// edited by. Made when the arc is selected, seeded with its range.
struct SelectedArc {
    id: ArcId,
    range: Entity<SliderField>,
    _range_changed: Subscription,
}

/// What releasing a mouse button over the viewport does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ClickAction {
    SnapGizmo,
    Cast,
    Burst,
    TogglePlate,
    Nothing,
}

/// Whether the armor's openings are marked, and how many were found.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct GapState {
    pub(crate) shown: bool,
    pub(crate) count: usize,
}

/// How far outside the hull a cast ray starts, in model units, so it crosses
/// every plate on the way in rather than only those past the point clicked.
const CAST_STANDOFF: f32 = 50.0;

/// How a cast arc is drawn over the scene: its line, the line of the arc
/// being edited, how far the dark outline under a line reaches past each
/// edge, and the plate and burst markers.
const ARC_LINE_WIDTH: Pixels = px(3.0);
const SELECTED_ARC_LINE_WIDTH: Pixels = px(5.0);
const ARC_OUTLINE: Pixels = px(1.5);
const IMPACT_RADIUS: Pixels = px(4.5);
const BURST_RADIUS: Pixels = px(7.0);

/// How wide a ship is assumed to be before its bounds are known, so an arc
/// drawn early has a length rather than none.
const DEFAULT_MODEL_EXTENT: f32 = 20.0;

/// The range a cast shell is fired from until one is chosen.
const DEFAULT_CAST_RANGE: Km = Km::new(10.0);

/// The span a cast range is chosen over, in kilometres: the egui viewer's
/// own range slider.
const CAST_RANGE_SLIDER: SliderRange = SliderRange::new(0.0, 30.0, 0.5);

/// Whether clicks burst shells, and what the last burst reached.
pub(crate) struct SplashState {
    pub(crate) shown: bool,
    pub(crate) boxes_shown: bool,
    /// Whether this hull ships a splash file at all. The mode is refused
    /// without one rather than placing a burst against nothing.
    pub(crate) has_data: bool,
    pub(crate) zones_reached: usize,
}

/// How many shells have been cast, and whether a ricochet is followed.
#[derive(Clone, Copy)]
pub(crate) struct TrajectoryState {
    pub(crate) count: usize,
    pub(crate) continue_on_ricochet: bool,
}

/// What a pane is currently showing all of.
///
/// Kept in the shared `armor_viewer_defaults` row, which is how a ship opened
/// after one where the reader hid everything opens the same way.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct AllVisible {
    pub(crate) hull: bool,
    pub(crate) armor: bool,
}

/// Which layer of a model is in view when it opens.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OpeningLayer {
    /// The armor, with the hull as the saved defaults leave it.
    #[default]
    Armor,
    /// The hull meshes, with the armor loaded but hidden.
    Mesh,
}

impl OpeningLayer {
    pub fn from_show_armor(show_armor: bool) -> Self {
        if show_armor { Self::Armor } else { Self::Mesh }
    }

    pub fn shows_armor(self) -> bool {
        self == Self::Armor
    }

    /// What `scene` opens on. A scene with no armor, or with armor of unknown
    /// thickness, opens on its meshes whatever the setting: there is nothing,
    /// or only uniform grey plates, to look at otherwise.
    pub fn for_scene(self, scene: &LoadedShipArmor) -> Self {
        if scene.has_armor() && !scene.thickness_unknown() { self } else { Self::Mesh }
    }
}

/// The visibility a newly opened model starts with.
///
/// An absent part is shown and an absent hull mesh is hidden (see the
/// `ViewportView` field docs), so only the other outcome gets entries.
#[derive(Debug, Default, PartialEq, Eq)]
struct OpeningVisibility {
    parts: HashMap<(String, String), bool>,
    hull: HashMap<String, bool>,
}

impl OpeningVisibility {
    fn new(armor: &LoadedShipArmor, saved: AllVisible, layer: OpeningLayer) -> Self {
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
struct ShownLayers {
    armor: bool,
    hull: bool,
}

/// Where the camera starts on `armor`, in a view `aspect` wide per unit of
/// height.
///
/// A ship is framed as the egui app frames it. A model is framed on the
/// layers drawn, so a hidden layer that reaches further does not push the
/// model shown into the distance.
fn opening_camera(armor: &LoadedShipArmor, shown: ShownLayers, aspect: f32) -> ArcballCamera {
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

/// Which visibility maps still hold the `OpeningLayer::Mesh` seed, untouched
/// by the reader. A seeded map stands for the saved default rather than a
/// choice: `all_visible` reports the default for it, and a reload seeds the
/// reloaded model's parts the same way.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct MeshSeed {
    armor: bool,
    hull: bool,
}

/// How far the hull may be heeled over from the display popover, in
/// degrees, matching the egui slider's own range.
const ROLL_LIMIT_DEG: f32 = 25.0;

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

/// How many lines of the shot log are shown at once.
const SHOT_LOG_LINES: usize = 12;

/// Offset (in each axis) from the cursor to the thickness tooltip's anchor
/// position, so the tooltip doesn't sit directly under the pointer.
const TOOLTIP_CURSOR_OFFSET: Pixels = px(16.);

/// Cross-pane events (Milestone 5 Task 9c): `pane.rs` subscribes to every
/// pane's `ViewportView` and, when `mirror_cameras`/`sync_options` is on,
/// broadcasts the source pane's camera/settings to every OTHER pane. Fired
/// only from genuinely USER-driven mutations (see each emit site's doc); the
/// silent counterparts [`ViewportView::set_camera`]/[`ViewportView::apply_synced`]
/// never emit, which is what keeps the broadcast from echoing back into a loop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ViewportChange {
    Camera,
    Settings,
}

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
    PanLeft,
    PanRight,
    PanUp,
    PanDown,
}

impl MoveKey {
    /// Shift turns the arrows from a vertical move and an orbit into a pan.
    fn from_key(key: &str, shift: bool) -> Option<Self> {
        match (key, shift) {
            ("w", _) => Some(Self::Forward),
            ("s", _) => Some(Self::Back),
            ("a", _) => Some(Self::Left),
            ("d", _) => Some(Self::Right),
            ("up", false) => Some(Self::Up),
            ("down", false) => Some(Self::Down),
            ("left", false) => Some(Self::RotateLeft),
            ("right", false) => Some(Self::RotateRight),
            ("up", true) => Some(Self::PanUp),
            ("down", true) => Some(Self::PanDown),
            ("left", true) => Some(Self::PanLeft),
            ("right", true) => Some(Self::PanRight),
            _ => None,
        }
    }

    /// Every motion a released key may have started: Shift can be let go
    /// before the arrow it modified.
    fn released_by(key: &str) -> impl Iterator<Item = Self> {
        [Self::from_key(key, false), Self::from_key(key, true)].into_iter().flatten()
    }
}

/// One tick's camera motion from the held keys, each axis summed over the
/// keys that drive it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct HeldMotion {
    forward: f32,
    right: f32,
    vertical: f32,
    rotate: f32,

    /// Screen space, +x right and +y up.
    pan_x: f32,
    pan_y: f32,
}

impl HeldMotion {
    fn from_keys<'a>(keys: impl IntoIterator<Item = &'a MoveKey>) -> Self {
        let mut motion = Self::default();
        for key in keys {
            match key {
                MoveKey::Forward => motion.forward += 1.0,
                MoveKey::Back => motion.forward -= 1.0,
                MoveKey::Left => motion.right -= 1.0,
                MoveKey::Right => motion.right += 1.0,
                MoveKey::Up => motion.vertical += 1.0,
                MoveKey::Down => motion.vertical -= 1.0,
                MoveKey::RotateLeft => motion.rotate += 1.0,
                MoveKey::RotateRight => motion.rotate -= 1.0,
                MoveKey::PanLeft => motion.pan_x -= 1.0,
                MoveKey::PanRight => motion.pan_x += 1.0,
                MoveKey::PanUp => motion.pan_y += 1.0,
                MoveKey::PanDown => motion.pan_y -= 1.0,
            }
        }

        motion
    }
}

/// Whether this viewport has GPUI's device to draw on.
enum GpuState {
    /// The pipeline is compiling, or GPUI's device is lost and not yet
    /// replaced.
    Waiting,
    /// Weak: the pane holds the device, and drops it, with the target and
    /// mesh buffers made on it, as soon as GPUI retires it.
    Ready(Weak<SceneDevice>),
    Failed(String),
}

impl GpuState {
    /// The device another viewport of the pane already built, so a compare
    /// pane can upload at once. None is built here: a pane whose tab is never
    /// opened compiles no pipelines; the first render starts the build.
    fn from_shared(shared: &SharedSceneDevice, viewport: &Viewport3D) -> Self {
        match shared.existing() {
            Some(scene) => {
                scene.track(viewport);
                Self::Ready(Rc::downgrade(&scene))
            }
            None => Self::Waiting,
        }
    }
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

/// The mesh under the pointer when no plate is: which mesh, what its tooltip
/// says, and the cursor position the tooltip is anchored near.
struct MeshHover {
    mesh: HullMeshIndex,
    tooltip: MeshTooltip,
    cursor: Point<Pixels>,
}

/// The display-settings popover's numeric sliders (`popover.rs`). A
/// persistent `Entity<SliderField>` per slider so
/// its thumb position survives the popover closing and reopening (the
/// popover's `.content()` closure only runs while open, see `popover.rs`'s
/// module doc); rebuilt wholesale whenever the seed changes outside a drag
/// (`apply_armor_defaults`), since moving a slider needs a `&mut Window` that
/// isn't available there -- mirrors `app.rs`'s own `zoom_slider`
/// reseed-by-recreation.
pub(crate) struct DisplaySettingsSliders {
    pub(crate) armor_opacity: Entity<SliderField>,
    /// How solid the impact markers are drawn.
    pub(crate) marker_opacity: Entity<SliderField>,
    /// How wide the locked camera sees, in degrees.
    pub(crate) perspective_fov: Entity<SliderField>,
    /// How far the hull is heeled over, in degrees. A property of the model
    /// on screen rather than of the armor, so it is not written back with the
    /// display defaults.
    pub(crate) model_roll_deg: Entity<SliderField>,
    /// Where between the field-of-view extremes the drawn camera orbits sit.
    pub(crate) camera_fov: Entity<SliderField>,
    /// How far those orbits are raised.
    pub(crate) camera_height: Entity<SliderField>,
    /// The range a cast shell is fired from, in kilometres, which sets how
    /// steeply it comes down onto the armor.
    pub(crate) cast_range: Entity<SliderField>,
}

/// The display-settings popover's lighting sliders, same persistent-entity
/// rationale as [`DisplaySettingsSliders`]. Rebuilt wholesale by a preset
/// button (`ViewportView::set_lighting_preset`), which moves every slider at
/// once.
/// The two colours the lighting is made of, as the pickers that set them.
///
/// Built with the view rather than with the popover: a picker keeps its own
/// sliders and its open state, and one rebuilt per frame would close itself as
/// the reader dragged it.
pub(crate) struct LightingColors {
    pub(crate) flat: Entity<gpui_kit::component::color_picker::ColorPickerState>,
    pub(crate) key: Entity<gpui_kit::component::color_picker::ColorPickerState>,
}

pub(crate) struct LightingSliders {
    pub(crate) flat_intensity: Entity<SliderField>,
    pub(crate) key_intensity: Entity<SliderField>,
    pub(crate) azimuth_deg: Entity<SliderField>,
    pub(crate) elevation_deg: Entity<SliderField>,
    pub(crate) rim_strength: Entity<SliderField>,
    pub(crate) specular_strength: Entity<SliderField>,
    pub(crate) shininess: Entity<SliderField>,
}

/// Which of the loads a pane started into one viewport a result belongs to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct LoadGeneration(u64);

/// A load the pane started into one viewport, and the flag that stops it.
#[derive(Clone, Debug)]
pub(crate) struct OpeningLoad {
    pub(crate) generation: LoadGeneration,
    pub(crate) cancel: LoadCancel,
}

/// Which kinds of option the scene on screen has anything for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SceneFeatures {
    /// A ship: splash boxes, camera orbits, the waterline, camo and hulls.
    pub(crate) ship: bool,
    /// Armor plates: their tree, the legend, casts and the plate tools.
    pub(crate) armor: bool,
}

/// What a pane's scene was loaded from, which a reload or an export reads
/// again. Set by the pane right before `show_armor`; a reload never replaces
/// it, only a new selection does.
#[derive(Clone)]
pub(crate) enum SceneSource {
    Ship { bundle: Arc<ArmorAssetsBundle>, param_index: String, display_name: String },
    Model { bundle: Arc<ArmorAssetsBundle>, models: Arc<ModelAssets>, request: ModelLoadRequest },
}

impl SceneSource {
    pub(crate) fn bundle(&self) -> &Arc<ArmorAssetsBundle> {
        match self {
            Self::Ship { bundle, .. } | Self::Model { bundle, .. } => bundle,
        }
    }

    pub(crate) fn display_name(&self) -> &str {
        match self {
            Self::Ship { display_name, .. } => display_name,
            Self::Model { request, .. } => request.path.stem(),
        }
    }

    /// The ship's param index, for a ship.
    pub(crate) fn param_index(&self) -> Option<&str> {
        match self {
            Self::Ship { param_index, .. } => Some(param_index),
            Self::Model { .. } => None,
        }
    }
}

/// What one pane hands another while "sync options" is on: everything
/// [`ViewportView::synced_settings`] and [`ViewportView::apply_synced`] carry
/// between them, matching the egui app's own `SyncedPaneSettings`
/// (`armor_viewer/state.rs`).
///
/// The selected camo is not among them: a `CamoSchemeId` is a stable index
/// within one ship's own scheme list (see that type's doc), so it cannot be
/// reapplied to a pane showing another ship without the remap
/// `apply_reload_result` does for a same-pane reload.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SyncedSettings {
    part_visibility: HashMap<(String, String), bool>,
    plate_visibility: HashMap<PlateKey, bool>,
    hull_visibility: HashMap<String, bool>,
    display_settings: upload::DisplaySettings,
    lighting: LightingSettings,
    camera_rings: CameraRingSettings,
    /// Whether the camera is locked to the ship's orbit, and how it aims.
    perspective: PerspectiveSettings,
    mesh_seed: MeshSeed,
}

pub struct ViewportView {
    focus_handle: FocusHandle,
    /// This view, which its target on the device is kept under.
    entity_id: EntityId,
    viewport: Viewport3D,
    /// GPUI's device as this pane's viewports share it.
    scene_devices: SharedSceneDevice,
    gpu: GpuState,
    /// World-space bounding box of the scene.
    model_bounds: Option<(Vec3, Vec3)>,
    /// The camera the scene opened with, which a double-click returns to.
    home_camera: Option<ArcballCamera>,
    /// Whether a standalone model's meshes are drawn opaque. Kept apart from
    /// `display_settings.hull_opaque`, the saved setting for ships, because a
    /// lone model has nothing inside it to see through to.
    model_hull_opaque: bool,
    /// The container's window-space bounds as of the last paint. `None` until
    /// the first layout pass; every screen-space projection (gizmo hit test,
    /// picking) waits on it.
    last_bounds: Option<Bounds<Pixels>>,
    drag: Option<DragState>,
    /// Where the pointer last moved over the view, so a hover can be picked
    /// again when the picking index lands under a still pointer.
    last_pointer: Option<Point<Pixels>>,
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
    /// A ship picked in the sidebar while GPUI's device was unavailable.
    /// `sync_gpu` uploads this once the device is back, then clears it.
    pending_armor: Option<(Arc<LoadedShipArmor>, OpeningLayer)>,
    mesh_seed: MeshSeed,
    /// The ship being loaded into this viewport, if one is. Set by the pane
    /// (`set_ship_loading`), which owns the load; the viewport shows it in
    /// place of whatever it was drawing, matching the egui pane
    /// (`ui/tab.rs:2081`).
    ship_loading: Option<SharedString>,
    /// Why the scene asked for cannot be drawn, shown in its place.
    scene_problem: Option<SceneProblem>,
    /// The latest load the pane started into this viewport.
    load_generation: LoadGeneration,
    /// The first stage of the load the pane started into this viewport.
    opening_load: LoadSlot,
    /// The first stage of the reload running for the scene shown.
    reload_load: LoadSlot,
    /// The background stage finishing the scene shown, shared with every
    /// viewport showing it.
    rest_load: Option<Arc<SharedLoad>>,
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
    /// Persisted `ArmorViewerDefaultsRow.armor_all_visible`, read only when a
    /// new ship loads (`upload_armor_now`) to seed `part_visibility`. Set by
    /// `apply_armor_defaults`; defaults to `true` (matching
    /// `ArmorViewerDefaults::default()`) before any row loads. Does not
    /// retroactively reseed an already-loaded ship's visibility -- carrying
    /// the previous ship's end-state into the next ship is a deferred
    /// nuance, not in this fix's scope.
    armor_all_visible: bool,
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
    /// Persisted `ArmorViewerDefaultsRow.hull_all_visible`, read only when a
    /// new ship loads (`upload_armor_now`) to seed `hull_visibility`. Set by
    /// `apply_armor_defaults`; defaults to `false` (matching
    /// `ArmorViewerDefaults::default()`) before any row loads. Same
    /// deferred-reseed scope note as `armor_all_visible`.
    hull_all_visible: bool,
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
    /// What a background reload (`reload_ship`) or an export reads again.
    /// `None` until the pane's first load completes (`set_scene_source`); a
    /// reload never clears or replaces this itself.
    scene_source: Option<SceneSource>,
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
    camo_texture_cache: CamoTextureCache,
    /// Bumped by every camo selection and by every armor swap, so a decode
    /// that lands after the reader moved on (or after the ship changed under
    /// it) is dropped rather than applied to the wrong hull.
    camo_generation: u64,
    /// The active camo's composited hull textures, keyed by mfm stem
    /// (`camo::build_active_camo`'s output). Empty when `selected_camo` is
    /// `None`. Lives here (not on `LoadedShipArmor`, an immutable `Arc`) and
    /// is passed into [`upload_hull::upload_hull_meshes`] as a parameter on
    /// every hull upload. Reset (cleared) whenever a new ship loads.
    active_camo_textures: CamoTextures,
    /// The active camo's UV transforms, keyed by mfm stem, for stems the
    /// compositor left tiled (GPU-side) rather than baking. Same lifetime/
    /// reset rules as `active_camo_textures`.
    active_camo_uvs: HashMap<String, UvTransform>,
    /// The GPU textures the hull meshes sample, one per material. Cleared
    /// with the scene, the active camo and the device they were made for.
    hull_textures: HullTextureCache,
    /// Whose hardpoints are marked, as the sidebar's checkboxes left them.
    /// Kept across scenes: a target only draws while its model or ship is
    /// the one shown.
    hardpoint_targets: HashSet<HardpointTarget>,
    /// The marked hardpoints' names and where each is drawn, for the
    /// labels painted over the scene.
    hardpoint_labels: Vec<hardpoint_overlay::HardpointLabel>,
    /// The marks on the grid under the shown scene with their distances
    /// written out, read for their labels and the overall size. `None`
    /// without a scene of any extent.
    length_marks: Option<MarkLabels>,
    /// The overlay mesh drawing the markers of `hardpoint_labels`.
    hardpoint_mesh: Option<MeshId>,
    /// The scene units per pixel `hardpoint_mesh` was sized for.
    hardpoint_world_per_pixel: f32,
    /// The marker under the pointer, by its index in `hardpoint_labels`.
    hovered_hardpoint: Option<usize>,
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
    /// What this ship had taken when a replay viewport asked for it, drawn
    /// over the hull as the overlays are.
    hits: Vec<wows_replay_insights::timeline::PreExtractedHit>,
    /// Rings still spreading from hits that landed as a battle played.
    hit_pulses: Vec<depth_cues::HitPulse>,
    /// What this app's own simulation made of each landed shell, against what
    /// the server said. Keyed by the shot, because that is what the log lists.
    sim_verdicts: HashMap<ShotId, SimAgreement>,
    /// How many armour-piercing shells the comparison could not reach an answer
    /// on. Counted rather than left as a gap in the log: a blank line beside a
    /// shell reads as nothing to say about it, and the reader should be able to
    /// tell that from the check not having run.
    sim_unchecked: usize,
    /// What the ship's health was at that moment, when a replay is driving
    /// this viewer. `None` when nothing is.
    hit_health: Option<f32>,
    /// The camera orbits currently drawn, so a pointer resting on one can be
    /// told which it is. Rebuilt with the overlay that drew them.
    ring_hovers: Vec<camera_rings::RingHover>,
    /// What the orbit under the pointer reads as, and where to anchor it.
    /// An orbit takes the tooltip from the plate behind it, which still
    /// picks for a click.
    hovered_ring: Option<(SharedString, Point<Pixels>)>,
    /// The overlay mesh highlighting `hovered`'s plate, if any, so it can be
    /// removed/replaced when the hovered plate changes.
    hover_highlight: Option<(PlateKey, MeshId)>,
    /// The overlay mesh highlighting a hovered Zone/Part/Plate row in the
    /// visibility popover, if any. Tracked separately from `hover_highlight`
    /// (raycast-driven) so the two don't fight each other; cleared when the
    /// hovered row changes, the popover closes, or a visibility mutation
    /// re-uploads the armor.
    sidebar_highlight: Option<(SidebarHighlightKey, MeshId)>,
    /// The mesh under the pointer when no plate is. A plate hit takes the
    /// hover from any mesh, so the plate tools keep working through the hull.
    hovered_mesh: Option<MeshHover>,
    /// The overlay mesh highlighting every mesh named as `hovered_mesh` is,
    /// by that name.
    mesh_highlight: Option<(String, MeshId)>,
    /// Which hull meshes the last hull upload drew, by [`HullMeshIndex`]:
    /// the meshes a hover can hit.
    hull_shown: Vec<bool>,
    /// Per-mesh triangle tooltip data from the most recent armor upload, used
    /// to map a `pick()` hit back to its `ArmorTriangleTooltip`.
    mesh_triangle_info: Vec<(MeshId, Vec<ArmorTriangleTooltip>)>,
    /// The zone and thickness of the last plate the pointer was over, kept
    /// after the hover ends so the penetration checker opens on it.
    last_plate: Option<(SharedString, f32)>,
    /// The pane this viewport is one of. Weak: the pane owns the dock that
    /// owns this. Carried so the viewport's context menu can offer the pane's
    /// own controls -- the options rail and the penetration checker.
    pane: Option<WeakEntity<ArmorViewerPane>>,
    /// Live display settings (plate edges, zero-mm plates, armor
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
    /// Which camera orbits are drawn over the ship, and how they are asked
    /// for. `mode` names one of the loaded ship's own trajectories; it is
    /// corrected to a mode the ship has whenever the armor changes, since a
    /// mode is a per-ship name rather than a global one.
    camera_rings: CameraRingSettings,
    /// Whether the camera is locked to one of the ship's own orbits, and where
    /// on it the eye sits.
    perspective: PerspectiveSettings,
    /// The free camera as it was when the lock went on, put back when it comes
    /// off: a reader who was looking at a plate is looking at it again.
    camera_before_lock: Option<crate::viewport::camera::ArcballCamera>,
    /// The meshes marking where the locked camera is aimed, dropped and rebuilt
    /// as it moves.
    aim_mesh_ids: Vec<crate::viewport::types::MeshId>,
    /// Whether the openings in the armor are marked, and how many were
    /// found last time they were looked for. The count is what the rail
    /// reports, so it is kept rather than recomputed per frame.
    show_gaps: bool,
    /// The shells cast so far, one per Shift-click on the hull. Several are
    /// kept at once, which is how two angles are compared.
    arcs: Vec<CastArc>,
    /// The id the next cast arc takes.
    next_arc: ArcId,
    /// The arc the trajectory section is editing.
    selected_arc: Option<SelectedArc>,
    /// Whether a shell that ricochets is followed past the bounce.
    continue_on_ricochet: bool,
    /// The ship and shell the next cast is fired with, pushed in by the pane,
    /// which owns the comparison list. `None` until one is chosen, which
    /// still casts: the plates a ray crosses are worth showing without a
    /// shell.
    cast_shell: Option<CastShell>,
    /// The range the next cast is fired from, which sets how steeply it
    /// falls.
    cast_range: Km,
    /// Whether a left click bursts a shell instead of hiding the plate under
    /// it.
    splash_mode: bool,
    /// The burst on screen, and what it reached. `None` until one is placed.
    splash_result: Option<splash::SplashResult>,
    /// Whether the named boxes are outlined over the hull. Independent of the
    /// mode: the outlines are worth seeing while placing a burst and after.
    show_splash_boxes: bool,
    /// The boxes' names and where each is written, while they are outlined.
    /// Built with the outlines, in `upload_splash_boxes`.
    splash_labels: Vec<splash::SplashBoxLabel>,
    gap_count: usize,
    /// Whether the viewport is showing only the plates the game's own armor
    /// viewer hides. A mode rather than a setting: it is not written back
    /// with the display defaults, because leaving it on is not a state to
    /// open the next ship in.
    show_hidden_only: bool,
    /// The display section's slider state.
    pub(crate) display_sliders: DisplaySettingsSliders,
    /// Kept alive so `display_sliders`' change subscriptions
    /// keep firing; replaced wholesale (dropping and canceling the old ones)
    /// whenever `apply_armor_defaults` rebuilds the sliders.
    _display_slider_subscriptions: Vec<Subscription>,
    /// The display-settings popover's lighting slider state.
    pub(crate) lighting_sliders: LightingSliders,
    /// The ambient and key light colours, which the shader multiplies each term
    /// by; the sliders beside them set how much of each there is.
    pub(crate) lighting_colors: LightingColors,
    /// Kept alive so `lighting_sliders`' change subscriptions
    /// keep firing; replaced wholesale whenever `set_lighting_preset` rebuilds
    /// the sliders.
    _lighting_slider_subscriptions: Vec<Subscription>,
    _lighting_color_subscriptions: Vec<Subscription>,
    /// The export-confirm panel (Milestone 5 Task 10) target, if
    /// open: a snapshot of everything the export needs, taken at the moment
    /// the panel opens (`open_export_confirm`), mirroring the egui app's own
    /// `Option<ExportRequest>` (`state.rs:180-188`, `tab.rs:556-576`). The
    /// panel stays interactive to the rest of the pane while open -- the
    /// sidebar can still be clicked -- so reading this snapshot rather than
    /// live `reload_source`/`selected_hull`/`hull_lod`/`selected_modules` is
    /// what keeps a sidebar re-click from silently retargeting an
    /// already-open "Export {A}?" confirmation onto a different ship B.
    export_confirm: Option<ExportConfirm>,
}

/// [`ViewportView::export_confirm`]'s snapshot: fixes the export target for
/// the lifetime of the confirm panel, taken from `reload_source` and the
/// pane's live hull/LOD/module selection at `open_export_confirm` time. Never
/// re-read from `self` after that -- `confirm_export` builds `ShipExportOptions`
/// from these fields, not from whatever the pane's live selection has become
/// by the time the user clicks "Export".
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
    pub fn new(scene_devices: SharedSceneDevice, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();
        let display_settings = upload::DisplaySettings::default();
        let (display_sliders, display_slider_subscriptions) = Self::build_display_sliders(cx, display_settings);
        let lighting = LightingSettings::default();
        let (lighting_sliders, lighting_slider_subscriptions) = Self::build_lighting_sliders(cx, &lighting);
        let (lighting_colors, lighting_color_subscriptions) = Self::build_lighting_colors(window, cx, &lighting);
        let viewport = Viewport3D::new();
        let gpu = GpuState::from_shared(&scene_devices, &viewport);
        // The target lives on the device; a closed viewport gives it back.
        cx.on_release(|this: &mut Self, _| {
            if let Some(scene) = this.scene() {
                scene.forget_view(this.entity_id);
            }
        })
        .detach();
        Self {
            focus_handle,
            entity_id: cx.entity_id(),
            viewport,
            gpu,
            scene_devices,
            model_bounds: None,
            home_camera: None,
            model_hull_opaque: true,
            last_bounds: None,
            drag: None,
            last_pointer: None,
            gizmo_hover: None,
            gizmo_press_in_box: false,
            animating: false,
            held_keys: HashSet::new(),
            key_ticking: false,
            sim_verdicts: HashMap::new(),
            sim_unchecked: 0,
            pending_armor: None,
            mesh_seed: MeshSeed::default(),
            ship_loading: None,
            scene_problem: None,
            load_generation: LoadGeneration::default(),
            opening_load: LoadSlot::default(),
            reload_load: LoadSlot::default(),
            rest_load: None,
            current_armor: None,
            camera_rings: CameraRingSettings::default(),
            perspective: PerspectiveSettings::default(),
            camera_before_lock: None,
            aim_mesh_ids: Vec::new(),
            show_gaps: false,
            arcs: Vec::new(),
            next_arc: ArcId::FIRST,
            selected_arc: None,
            continue_on_ricochet: false,
            cast_shell: None,
            cast_range: DEFAULT_CAST_RANGE,
            splash_mode: false,
            splash_result: None,
            show_splash_boxes: false,
            splash_labels: Vec::new(),
            gap_count: 0,
            show_hidden_only: false,
            part_visibility: HashMap::new(),
            armor_all_visible: true,
            plate_visibility: HashMap::new(),
            hull_visibility: HashMap::new(),
            hull_all_visible: false,
            hull_mesh_ids: Vec::new(),
            expanded_hull_groups: HashSet::new(),
            expanded_camo_groups: HashSet::new(),
            scene_source: None,
            selected_hull: None,
            hull_lod: load_ship::DEFAULT_LOD,
            selected_modules: HashMap::new(),
            reload_generation: 0,
            selected_camo: None,
            camo_texture_cache: CamoTextureCache::default(),
            camo_generation: 0,
            active_camo_textures: HashMap::new(),
            active_camo_uvs: HashMap::new(),
            hull_textures: HullTextureCache::default(),
            hardpoint_targets: HashSet::new(),
            hardpoint_labels: Vec::new(),
            length_marks: None,
            hardpoint_mesh: None,
            hardpoint_world_per_pixel: 0.0,
            hovered_hardpoint: None,
            undo_stack: VisibilityUndoStack::default(),
            expanded_zones: HashSet::new(),
            expanded_parts: HashSet::new(),
            popover_scroll: ScrollHandle::new(),
            hovered: None,
            hits: Vec::new(),
            hit_pulses: Vec::new(),
            hit_health: None,
            ring_hovers: Vec::new(),
            hovered_ring: None,
            hover_highlight: None,
            sidebar_highlight: None,
            hovered_mesh: None,
            mesh_highlight: None,
            hull_shown: Vec::new(),
            mesh_triangle_info: Vec::new(),
            last_plate: None,
            pane: None,
            display_settings,
            display_sliders,
            _display_slider_subscriptions: display_slider_subscriptions,
            lighting_sliders,
            lighting_colors,
            _lighting_color_subscriptions: lighting_color_subscriptions,
            _lighting_slider_subscriptions: lighting_slider_subscriptions,
            export_confirm: None,
        }
    }

    /// A slider over `range` showing `value`, which its reset button puts
    /// back to `reset_to`. For
    /// [`build_display_sliders`](Self::build_display_sliders)/
    /// [`build_lighting_sliders`](Self::build_lighting_sliders).
    fn new_slider(cx: &mut Context<Self>, range: SliderRange, value: f32, reset_to: f32) -> Entity<SliderField> {
        cx.new(|cx| SliderField::new(range, value, reset_to, cx))
    }

    /// Subscribes so every change on `state` (fired continuously while the
    /// user drags the thumb, matching egui's own per-frame
    /// `Slider::changed()`, and once for a typed value or a reset) invokes
    /// `on_change` with the new value.
    fn subscribe_slider(
        cx: &mut Context<Self>,
        state: &Entity<SliderField>,
        on_change: impl Fn(&mut Self, f32, &mut Context<Self>) + 'static,
    ) -> Subscription {
        cx.subscribe(state, move |this, _state, SliderFieldChange(value), cx| on_change(this, *value, cx))
    }

    /// Builds [`DisplaySettingsSliders`] seeded from `display`, wired so a
    /// drag calls [`mutate_display_settings`](Self::mutate_display_settings).
    /// Called from `new()` and, to reseed after `apply_armor_defaults` loads
    /// a persisted row, again there.
    fn build_display_sliders(
        cx: &mut Context<Self>,
        display: upload::DisplaySettings,
    ) -> (DisplaySettingsSliders, Vec<Subscription>) {
        // What each reset puts back: the settings a fresh install starts on.
        let factory = upload::DisplaySettings::default();
        let armor_opacity = Self::new_slider(
            cx,
            const { SliderRange::new(0.1, 1.0, 0.01) },
            display.armor_opacity,
            factory.armor_opacity,
        );
        let marker_opacity = Self::new_slider(
            cx,
            const { SliderRange::new(0.0, 1.0, 0.01) },
            display.marker_opacity,
            factory.marker_opacity,
        );
        let perspective_fov = Self::new_slider(
            cx,
            const { SliderRange::new(30.0, 120.0, 1.0) },
            CameraPerspective::default().fov_deg,
            CameraPerspective::default().fov_deg,
        );
        // The range the egui slider uses: a hull heels this far in a hard
        // turn, and further than that reads as a capsize rather than a
        // camera angle worth checking armor against.
        let model_roll_deg =
            Self::new_slider(cx, const { SliderRange::new(-ROLL_LIMIT_DEG, ROLL_LIMIT_DEG, 0.5) }, 0.0, 0.0);
        // The ranges the trajectory itself is resolved over.
        let camera_fov = Self::new_slider(cx, const { SliderRange::new(0.0, 1.0, 0.01) }, 0.0, 0.0);
        let camera_height = Self::new_slider(cx, const { SliderRange::new(-1.0, 1.0, 0.01) }, 0.0, 0.0);
        let cast_range =
            Self::new_slider(cx, CAST_RANGE_SLIDER, DEFAULT_CAST_RANGE.value(), DEFAULT_CAST_RANGE.value());
        let subs = vec![
            Self::subscribe_slider(cx, &armor_opacity, |this, v, cx| {
                this.mutate_display_settings(cx, |d| d.armor_opacity = v)
            }),
            Self::subscribe_slider(cx, &marker_opacity, |this, v, cx| {
                this.mutate_display_settings(cx, |d| d.marker_opacity = v)
            }),
            Self::subscribe_slider(cx, &perspective_fov, |this, v, cx| this.set_perspective_fov(v, cx)),
            Self::subscribe_slider(cx, &model_roll_deg, |this, v, cx| this.set_model_roll_deg(v, cx)),
            Self::subscribe_slider(cx, &camera_fov, |this, v, cx| {
                let next = CameraRingSettings { fov: v, ..this.camera_rings.clone() };
                this.set_camera_rings(next, cx);
            }),
            Self::subscribe_slider(cx, &camera_height, |this, v, cx| {
                let next = CameraRingSettings { height: v, ..this.camera_rings.clone() };
                this.set_camera_rings(next, cx);
            }),
            Self::subscribe_slider(cx, &cast_range, |this, v, cx| this.set_cast_range(Km::new(v), cx)),
        ];
        (
            DisplaySettingsSliders {
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

    /// Builds [`LightingSliders`] seeded from `lighting`, wired so a drag
    /// calls [`mutate_lighting`](Self::mutate_lighting). Called from `new()`
    /// and, to move every thumb at once, again from `set_lighting_preset`.
    /// The two colour pickers, seeded from `lighting` and reporting every change
    /// back into it.
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
            this.mutate_lighting(cx, |lighting| apply(lighting, channels));
        })
    }

    fn build_lighting_sliders(
        cx: &mut Context<Self>,
        lighting: &LightingSettings,
    ) -> (LightingSliders, Vec<Subscription>) {
        let factory = LightingSettings::default();
        let flat_intensity = Self::new_slider(
            cx,
            const { SliderRange::new(0.0, 1.5, 0.01) },
            lighting.flat_intensity,
            factory.flat_intensity,
        );
        let key_intensity = Self::new_slider(
            cx,
            const { SliderRange::new(0.0, 1.5, 0.01) },
            lighting.key_intensity,
            factory.key_intensity,
        );
        let azimuth_deg = Self::new_slider(
            cx,
            const { SliderRange::new(0.0, 360.0, 1.0) },
            lighting.azimuth_deg,
            factory.azimuth_deg,
        );
        let elevation_deg = Self::new_slider(
            cx,
            const { SliderRange::new(-90.0, 90.0, 1.0) },
            lighting.elevation_deg,
            factory.elevation_deg,
        );
        let rim_strength = Self::new_slider(
            cx,
            const { SliderRange::new(0.0, 1.0, 0.01) },
            lighting.rim_strength,
            factory.rim_strength,
        );
        let specular_strength = Self::new_slider(
            cx,
            const { SliderRange::new(0.0, 1.0, 0.01) },
            lighting.specular_strength,
            factory.specular_strength,
        );
        let shininess =
            Self::new_slider(cx, const { SliderRange::new(1.0, 128.0, 1.0) }, lighting.shininess, factory.shininess);
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
    /// DisplaySettings::from_defaults`), rebuilds the display sliders so
    /// their thumbs reflect the new seed, and -- if a ship is already shown
    /// -- re-uploads it. Called once from `ArmorViewerPane::
    /// apply_armor_defaults`, itself called once from `App::apply_settings`.
    pub fn apply_armor_defaults(&mut self, defaults: Option<&ArmorViewerDefaultsRow>, cx: &mut Context<Self>) {
        self.display_settings = upload::DisplaySettings::from_defaults(defaults);
        self.armor_all_visible = defaults.map(|d| d.armor_all_visible).unwrap_or(true);
        self.hull_all_visible = defaults.map(|d| d.hull_all_visible).unwrap_or(false);
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
    /// The assets this pane's ship was loaded from, which is where the
    /// translations for part and material names come from.
    pub fn reload_bundle(&self) -> Option<Arc<ArmorAssetsBundle>> {
        self.scene_source.as_ref().map(|source| Arc::clone(source.bundle()))
    }

    /// The class of the ship shown, which picks the game's class-specific
    /// plate names.
    pub(crate) fn shown_species(&self) -> Option<Species> {
        self.current_armor.as_ref().and_then(|armor| armor.species)
    }

    /// `material` named the way the game names it on the ship shown, or by
    /// its English label while the build is still loading.
    pub(crate) fn material_label(&self, material: ArmorMaterial) -> String {
        match self.reload_bundle() {
            Some(bundle) => material.localized_name(bundle.assets.metadata(), self.shown_species()),
            None => material.english_label(),
        }
    }

    pub(crate) fn set_scene_source(&mut self, source: SceneSource) {
        self.scene_source = Some(source);
    }

    /// The scene last handed to this pane: the one waiting for GPUI's device,
    /// which is newer than the one on screen, or else the one on screen.
    pub(crate) fn shown(&self) -> Option<&Arc<LoadedShipArmor>> {
        self.pending_armor.as_ref().map(|(armor, _)| armor).or(self.current_armor.as_ref())
    }

    /// Which of `armor`'s layers the visibility maps draw any of. An absent
    /// part is drawn and an absent hull mesh is not (see the field docs).
    fn shown_layers(&self, armor: &LoadedShipArmor) -> ShownLayers {
        let armor_shown = armor.zone_parts.iter().any(|(zone, parts)| {
            parts.iter().any(|part| self.part_visibility.get(&(zone.clone(), part.clone())) != Some(&false))
        });
        let hull_shown = armor.hull_meshes.iter().any(|mesh| self.hull_visibility.get(&mesh.name) == Some(&true));

        ShownLayers { armor: armor_shown, hull: hull_shown }
    }

    /// What the scene handed to this pane has for the option rail to act on.
    /// `None` while nothing has been.
    pub(crate) fn scene_features(&self) -> Option<SceneFeatures> {
        self.shown().map(|armor| SceneFeatures { ship: armor.is_ship(), armor: armor.has_armor() })
    }

    /// What this pane's scene was loaded from. `pane.rs`'s Compare-clone uses
    /// this to seed a new pane's own `set_scene_source`, so the new pane can
    /// independently switch hull/LOD/module afterward.
    pub(crate) fn scene_source(&self) -> Option<SceneSource> {
        self.scene_source.clone()
    }

    /// The param index of the ship actually shown right now, or `None` if no
    /// ship has finished loading into this pane yet. Gated on `current_armor`
    /// (not just `scene_source`) so a load still in flight -- which has
    /// already called `set_scene_source` but hasn't reached `show_armor` --
    /// does not count as "loaded": `pane.rs`'s re-click guard (mirroring
    /// egui's `already_selected`, `ui/tab.rs:520-521`) only skips a reload
    /// once the previous one actually finished and is on screen.
    pub(crate) fn loaded_param_index(&self) -> Option<&str> {
        self.current_armor.as_ref()?;
        self.scene_source.as_ref()?.param_index()
    }

    /// The model or ship shown, for the sidebar to highlight. `None` while
    /// nothing has been handed to this pane.
    pub(crate) fn open_target(&self) -> Option<OpenTarget> {
        let target = match &self.shown()?.subject {
            SceneSubject::Ship(ship) => OpenTarget::Ship(ship.vehicle_param_index.clone()),
            SceneSubject::Model(model) => match model.from_building {
                Some(building) => OpenTarget::Building(building),
                None => OpenTarget::Model { model: model.path.clone(), from_ship: model.from_ship.clone() },
            },
        };

        Some(target)
    }

    /// A clone of the current camera, for `pane.rs` to read when broadcasting
    /// a camera-mirror update or seeding a Compare-cloned pane.
    pub(crate) fn camera(&self) -> ArcballCamera {
        self.viewport.camera.clone()
    }

    /// Sets the camera directly, with no `ViewportChange::Camera` emit.
    /// The silent counterpart to every user-driven camera mutation (orbit,
    /// pan, zoom, wasd, gizmo/double-click reset): `pane.rs`'s camera-mirror
    /// broadcast and the Compare-clone both call this on the RECEIVING
    /// pane(s), and emitting here would echo the change back out and loop.
    pub(crate) fn set_camera(&mut self, camera: ArcballCamera, cx: &mut Context<Self>) {
        self.viewport.camera = camera;
        self.viewport.mark_dirty();
        cx.notify();
    }

    /// Captures this pane's cross-pane-relevant settings (see
    /// [`SyncedSettings`]'s doc for exactly what's included/excluded), for
    /// `pane.rs` to broadcast via [`Self::apply_synced`] on every other pane.
    pub(crate) fn synced_settings(&self) -> SyncedSettings {
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

    /// Applies `settings` to this pane, with no `ViewportChange::Settings`
    /// emit and no undo-stack entry -- the silent counterpart to every
    /// visibility/display/hull mutator, matching the egui app's own
    /// `SyncedPaneSettings::apply_to` (also a direct field assignment with no
    /// undo tracking). Re-uploads armor and hull in one pass via
    /// `reupload_current_armor` (a no-op if this pane has no ship loaded), and
    /// marks the viewport dirty unconditionally so a lighting-only change (no
    /// mesh re-upload) still redraws.
    pub(crate) fn apply_synced(&mut self, settings: &SyncedSettings, cx: &mut Context<Self>) {
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
        // Through the toggle rather than by assignment: turning the lock on
        // keeps the free camera to put back, and turning it off puts it back.
        let locked = settings.perspective.enabled;
        self.perspective.camera = settings.perspective.camera;
        self.set_perspective_enabled(locked, cx);
        self.reupload_current_armor(cx);
        self.viewport.mark_dirty();
        cx.notify();
    }

    /// Rebuilds each map `mesh_seed` marks from `armor`'s own names.
    fn reseed_mesh(&mut self, armor: &LoadedShipArmor) {
        let saved = AllVisible { hull: self.hull_all_visible, armor: self.armor_all_visible };
        let reseed = OpeningVisibility::new(armor, saved, OpeningLayer::Mesh);
        if self.mesh_seed.armor {
            self.part_visibility = reseed.parts;
        }
        if self.mesh_seed.hull {
            self.hull_visibility = reseed.hull;
        }
    }

    /// Shows `armor`'s armor meshes in this viewport: clears any previous
    /// meshes, uploads the new ones (`upload::upload_armor_to_viewport`,
    /// which also frames the camera on the model's bounds and marks the
    /// viewport dirty), and updates `model_bounds` so double-click-to-reset
    /// frames the new ship. While GPUI's device is unavailable, stashes
    /// `armor` in `pending_armor` instead; `sync_gpu` uploads it once the
    /// device is back. `layer` picks what is in view once it is up.
    ///
    /// Bumps `reload_generation` and cancels any reload in flight against
    /// the scene replaced, so `apply_reload_result`'s generation guard
    /// discards its result, whether or not the device is there.
    pub fn show_armor(&mut self, armor: Arc<LoadedShipArmor>, layer: OpeningLayer, cx: &mut Context<Self>) {
        self.reload_generation += 1;
        self.reload_load.cancel();
        // What the background stage of the scene replaced would still add is
        // of no use now.
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

    /// Uploads `armor` into the viewport and keeps it as `current_armor` for
    /// later visibility-only re-uploads (`reupload_current_armor`). Resets
    /// `part_visibility`/`plate_visibility`/`hull_visibility` (then reseeds
    /// them from the persisted `armor_all_visible`/`hull_all_visible`
    /// defaults, see below), the hull/LOD/module selection (`selected_hull`/
    /// `hull_lod`/`selected_modules`), the undo/redo history, the popover's
    /// expanded-row state, and any hover state -- a freshly loaded ship
    /// starts at stock hull, highest-detail LOD, and nothing hovered/
    /// undoable, matching the egui app's own reset on ship load
    /// (`tab.rs:1453-1454`, `2393-2394`, `2741`). Uploads the hull too (a
    /// visual no-op the first time when `hull_all_visible` is `false`, but
    /// exercises the same path a later hull-visibility toggle uses). Callers
    /// must have already checked `self.gpu` is `Ready`; a no-op otherwise.
    fn upload_armor_now(&mut self, armor: Arc<LoadedShipArmor>, layer: OpeningLayer, cx: &mut Context<Self>) {
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
        // The arcs were cast at the plates of the ship before; on this one
        // they mark nothing.
        self.arcs.clear();
        self.selected_arc = None;
        let shown = self.shown_layers(&armor);
        let (width, height) = self.viewport_size();
        self.viewport.camera = opening_camera(&armor, shown, width / height.max(1.0));
        self.home_camera = Some(self.viewport.camera.clone());
        // The camera the lock puts back is the one framed on the ship now
        // showing, not the one framed on the ship before it. A ship that names
        // no orbit has nothing to lock onto, and its control is not drawn, so
        // the lock comes off rather than being left on with no way out.
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

    /// Outlines the named splash boxes while they are shown, and keeps their
    /// names for the overlay. The outlines are meshes and the names are not,
    /// so both are made here to stay in step.
    fn upload_splash_boxes(&mut self, device: &wgpu::Device, armor: &LoadedShipArmor) {
        self.splash_labels.clear();
        let Some(data) = armor.splash_data.as_ref().filter(|_| self.show_splash_boxes) else { return };

        let (vertices, indices) = splash::build_splash_box_wireframes(&data.boxes);
        if !indices.is_empty() {
            self.viewport.add_non_pickable_mesh(device, &vertices, &indices, LAYER_OVERLAY);
        }
        self.splash_labels = splash::box_labels(&data.boxes);
    }

    /// Marks the hardpoints the sidebar asked for that belong to the scene
    /// shown, and keeps their names for the labels. The names are kept
    /// without a device too, so they always say what is marked.
    fn upload_hardpoints(&mut self) {
        self.hovered_hardpoint = None;
        // Before the first paint the view's height is unknown, so no
        // on-screen floor is applied until it is painted.
        let world_per_pixel =
            self.last_bounds.map_or(0.0, |bounds| self.viewport.camera.world_per_pixel(bounds.size.height.as_f32()));
        self.build_hardpoints(world_per_pixel);
    }

    /// [`Self::upload_hardpoints`] with markers held to a least size of
    /// `world_per_pixel` per pixel.
    fn build_hardpoints(&mut self, world_per_pixel: f32) {
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
    pub(crate) fn set_hardpoint_targets(&mut self, targets: HashSet<HardpointTarget>, cx: &mut Context<Self>) {
        if self.hardpoint_targets == targets {
            return;
        }
        self.hardpoint_targets = targets;
        self.upload_hardpoints();
        cx.notify();
    }

    /// The marked hardpoints, as their labels name them.
    pub(crate) fn hardpoint_labels(&self) -> &[hardpoint_overlay::HardpointLabel] {
        &self.hardpoint_labels
    }

    /// Whether plates of 0 mm are drawn: when asked for, and always for a
    /// model whose plates have no known thickness at all.
    pub(crate) fn show_zero_mm(&self) -> bool {
        match self.shown() {
            Some(armor) => armor.effective_display(self.display_settings).show_zero_mm,
            None => self.display_settings.show_zero_mm,
        }
    }

    /// The device meshes are uploaded to, while there is one.
    fn scene(&self) -> Option<Rc<SceneDevice>> {
        match &self.gpu {
            GpuState::Ready(scene) => scene.upgrade(),
            GpuState::Waiting | GpuState::Failed(_) => None,
        }
    }

    /// Follows GPUI's device. On a new device, uploads whatever a ship
    /// selection stashed in `pending_armor` while there was none, or uploads
    /// the current ship again: meshes and targets made on an earlier device
    /// are released with it and cannot be drawn on this one.
    fn sync_gpu(&mut self, cx: &mut Context<Self>) {
        let current = self.scene_devices.current(self.entity_id, cx);
        if let (Ok(Some(scene)), Some(held)) = (&current, self.scene())
            && Rc::ptr_eq(scene, &held)
        {
            return;
        }

        if matches!(self.gpu, GpuState::Ready(_)) {
            self.viewport.clear();
        }
        self.gpu = match current {
            Ok(Some(scene)) => {
                scene.track(&self.viewport);
                GpuState::Ready(Rc::downgrade(&scene))
            }
            Ok(None) => GpuState::Waiting,
            Err(error) => GpuState::Failed(error.to_string()),
        };
        if !matches!(self.gpu, GpuState::Ready(_)) {
            return;
        }

        // The bind groups were made on the device being replaced.
        self.hull_textures.clear();
        let Some((armor, layer)) = self.pending_armor.take() else {
            self.reupload_current_armor(cx);
            return;
        };
        self.upload_armor_now(armor, layer, cx);
        // The comparison needs a hull, and the one it was asked for arrived
        // while there was no device, so it found none and did nothing.
        if let Some((taken, shells)) = self.pane.as_ref().and_then(|pane| pane.upgrade()).and_then(|pane| {
            let loaded = self.loaded_param_index().map(str::to_owned);
            pane.read(cx).incoming().to_compare(loaded.as_deref())
        }) {
            self.compare_incoming(&taken, &shells, cx);
        }
    }

    /// The ship this viewport is showing, if any. Read by the pane's chrome,
    /// which names it.
    /// Whether a ship's armor is loaded, which is what the pane-level controls
    /// in the toolbar are gated on.
    /// How far the hull is heeled over, in degrees.
    pub(crate) fn model_roll_deg(&self) -> f32 {
        self.viewport.model_roll.to_degrees()
    }

    /// Heels the hull over. Only the model turns: the waterline and the other
    /// world-space overlays stay level, which is what makes the angle
    /// readable.
    pub(crate) fn set_model_roll_deg(&mut self, degrees: f32, cx: &mut Context<Self>) {
        let radians = degrees.to_radians();
        if (self.viewport.model_roll - radians).abs() < f32::EPSILON {
            return;
        }
        self.viewport.model_roll = radians;
        self.viewport.mark_dirty();
        cx.notify();
    }

    /// Whether everything is currently shown, which is what the next ship
    /// loaded starts from.
    ///
    /// Read against this port's own absence rules rather than the egui app's:
    /// a part with no entry is visible and a hull mesh with no entry is not,
    /// so an empty `part_visibility` means every part is on while an empty
    /// `hull_visibility` means no hull mesh is.
    pub(crate) fn all_visible(&self) -> AllVisible {
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

    /// How the camera orbits are being drawn, and which modes this ship
    /// offers.
    pub(crate) fn camera_rings(&self) -> CameraRingSettings {
        self.camera_rings.clone()
    }

    /// The camera modes this ship's own GameParams name, in their own order.
    pub(crate) fn camera_modes(&self) -> Vec<String> {
        self.current_armor
            .as_ref()
            .map(|armor| armor.camera_trajectories.iter().map(|(name, _)| name.clone()).collect())
            .unwrap_or_default()
    }

    /// Adopts a change made in the display popover's camera-rings section.
    pub(crate) fn set_camera_rings(&mut self, settings: CameraRingSettings, cx: &mut Context<Self>) {
        if self.camera_rings == settings {
            return;
        }
        self.camera_rings = settings;
        self.reupload_current_armor(cx);
        cx.notify();
    }

    /// Where the camera lock stands, for the popover that steers it.
    pub(crate) fn perspective(&self) -> PerspectiveSettings {
        self.perspective
    }

    /// Locks the camera to the ship's own orbit, or lets it go again.
    ///
    /// The free camera is kept while the lock is on and put back when it comes
    /// off, and the lock starts where the free camera was looking from.
    pub(crate) fn set_perspective_enabled(&mut self, on: bool, cx: &mut Context<Self>) {
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
    pub(crate) fn set_perspective_fov(&mut self, degrees: f32, cx: &mut Context<Self>) {
        if (self.perspective.camera.fov_deg - degrees).abs() < f32::EPSILON {
            return;
        }
        self.perspective.camera.fov_deg = degrees;
        self.perspective.camera.clamp();
        self.reupload_current_armor(cx);
        cx.notify();
    }

    /// Whether the locked camera looks along its bearing as the game's does, or
    /// through the ship's centre.
    pub(crate) fn set_perspective_look_mode(&mut self, mode: LookMode, cx: &mut Context<Self>) {
        if self.perspective.camera.look_mode == mode {
            return;
        }
        self.perspective.camera.look_mode = mode;
        self.reupload_current_armor(cx);
        cx.notify();
    }

    /// The trajectory the locked camera and the drawn orbits both follow: the
    /// mode the reader picked, among the ones this ship names.
    fn current_trajectory(&self) -> Option<&wowsunpack::game_params::types::CameraTrajectory> {
        let armor = self.current_armor.as_ref()?;
        let wanted = self.camera_rings.mode.as_ref();
        armor
            .camera_trajectories
            .iter()
            .find(|(name, _)| Some(name) == wanted)
            .or_else(|| armor.camera_trajectories.first())
            .map(|(_, trajectory)| trajectory)
    }

    /// Points the viewport's camera where the lock says, and marks what it is
    /// aimed at.
    ///
    /// A no-op while the lock is off, and while the ship names no orbit to lock
    /// onto: there is nothing to put the eye on.
    fn apply_perspective(&mut self, device: &wgpu::Device, armor: &super::load_ship::LoadedShipArmor) {
        self.aim_perspective(Some(device), armor.bounds);
    }

    /// Points the camera where the lock says, for a change that moved the lock
    /// rather than the model.
    ///
    /// Cheap enough for a drag: the armor is not touched, only the camera and
    /// the one marker saying where it is aimed.
    fn refresh_perspective(&mut self, cx: &mut Context<Self>) {
        let scene = self.scene();
        let Some(bounds) = self.model_bounds else { return };
        self.aim_perspective(scene.as_deref().map(SceneDevice::device), bounds);
        self.viewport.mark_dirty();
        cx.notify();
    }

    /// Points the viewport's camera where the lock says, and marks what it is
    /// aimed at when there is a device to build the marker with.
    ///
    /// A no-op while the lock is off, and while the ship names no orbit to lock
    /// onto: there is nothing to put the eye on.
    fn aim_perspective(&mut self, device: Option<&wgpu::Device>, bounds: (Vec3, Vec3)) {
        for id in self.aim_mesh_ids.drain(..) {
            self.viewport.remove_mesh(id);
        }
        if !self.perspective.enabled {
            return;
        }
        let Some(trajectory) = self.current_trajectory().cloned() else { return };

        // The orbits sit at the height the trajectory states, as the drawn rings
        // do: the port's armor carries no waterline offset.
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
        // Near enough to stand on the ship without the hull clipping away, far
        // enough to keep the water the eye is aimed at in the frustum.
        camera.near = 0.05;
        camera.far = (diagonal * 8.0).max(10.0);

        let Some(device) = device else { return };
        let radius = (diagonal * AIM_MARKER_FRACTION).clamp(AIM_MARKER_MIN, AIM_MARKER_MAX);
        let (vertices, indices) = camera_rings::build_water_marker(target, radius, AIM_MARKER_COLOR);
        if !indices.is_empty() {
            let id = self.viewport.add_world_space_mesh(device, &vertices, &indices, LAYER_OVERLAY);
            self.aim_mesh_ids.push(id);
        }
    }

    /// Whether the free camera is the reader's to move.
    ///
    /// While the lock is on the camera belongs to the ship's orbit, and anything
    /// that moved it another way would be undone by the next aim.
    fn camera_is_free(&self) -> bool {
        !self.perspective.enabled
    }

    /// Turns the locked camera by a drag, in pixels. Says whether it moved.
    fn steer_perspective(&mut self, dx: f32, dy: f32) -> bool {
        if !self.perspective.enabled || (dx == 0.0 && dy == 0.0) {
            return false;
        }
        self.perspective.camera.yaw -= dx * PERSPECTIVE_YAW_PER_PX;
        self.perspective.camera.pitch += dy * PERSPECTIVE_PITCH_PER_PX;
        self.perspective.camera.clamp();
        true
    }

    /// Moves the locked camera between the inner and outer orbit. Says whether
    /// it moved.
    fn zoom_perspective(&mut self, notches: f32) -> bool {
        if !self.perspective.enabled || notches == 0.0 {
            return false;
        }
        self.perspective.camera.zoom -= notches * PERSPECTIVE_ZOOM_PER_NOTCH;
        self.perspective.camera.clamp();
        true
    }

    /// Draws what the ship had taken, replacing whatever was drawn before.
    ///
    /// A step backwards in playback cannot be undrawn hit by hit, so a
    /// viewport hands over the whole set each time rather than what changed.
    pub(crate) fn show_hits(
        &mut self,
        hits: Vec<wows_replay_insights::timeline::PreExtractedHit>,
        cx: &mut Context<Self>,
    ) {
        self.hits = hits;
        self.reupload_current_armor(cx);
        cx.notify();
    }

    /// Draws what the ship had taken as playback moves, sending a ring out
    /// from each hit that has landed since the last call.
    pub(crate) fn follow_hits(
        &mut self,
        hits: Vec<wows_replay_insights::timeline::PreExtractedHit>,
        cx: &mut Context<Self>,
    ) {
        /// A seek forward lands many hits at once; rings for all of them
        /// would cover the hull, so only the latest few get one.
        const MOST_RINGS: usize = 4;

        if let Some(armor) = self.current_armor.as_ref().filter(|_| !cx.reduce_motion()) {
            let center = armor.center();
            let center = [center.x, center.y, center.z];
            let bounds = armor.bounds;
            let bounds = Some(([bounds.0.x, bounds.0.y, bounds.0.z], [bounds.1.x, bounds.1.y, bounds.1.z]));
            let landed = depth_cues::landed(&self.hits, &hits, |hit| hit.clock);
            let started = cx.background_executor().now();
            let rings = landed
                .iter()
                .rev()
                .filter_map(|hit| {
                    let at = wows_replay_insights::hull_impact::hull_impact(&hit.hit, center, bounds)?;
                    Some(depth_cues::HitPulse { at: Vec3::new(at[0], at[1], at[2]), started })
                })
                .take(MOST_RINGS);
            self.hit_pulses.extend(rings);
        }

        self.show_hits(hits, cx);
    }

    /// Draws the rings still spreading from landed hits, and asks for the
    /// next frame while any is.
    fn paint_hit_pulses(&mut self, bounds: Bounds<Pixels>, window: &mut Window, cx: &mut App) {
        let now = cx.background_executor().now();
        if cx.reduce_motion() {
            self.hit_pulses.clear();
        }
        self.hit_pulses.retain(|pulse| now.saturating_duration_since(pulse.started) < depth_cues::PULSE);
        if self.hit_pulses.is_empty() {
            return;
        }

        let rect = view_rect_from_bounds(bounds);
        let color = rgb_color([super::hits::COLOR[0], super::hits::COLOR[1], super::hits::COLOR[2]]);
        for pulse in &self.hit_pulses {
            let anchor = self.viewport.pos_to_world_space(pulse.at);
            let Some(at) = self.viewport.camera.project_to_screen(anchor, rect).filter(|at| rect.contains(*at)) else {
                continue;
            };
            let progress = now.saturating_duration_since(pulse.started).as_secs_f32() / depth_cues::PULSE.as_secs_f32();
            depth_cues::paint_pulse(point(px(at.x), px(at.y)), progress, color, window);
        }
        window.request_animation_frame();
    }

    /// What this app's own simulation made of a landed shell.
    ///
    /// Absent for a shell that was not simulated: one fired with something other
    /// than armour-piercing, one whose salvo this build cannot name the shell
    /// of, one whose victim's pose was not held at that moment, and one whose
    /// server outcome the simulation does not model.
    pub(crate) fn sim_agreement(&self, shot: ShotId) -> Option<&SimAgreement> {
        self.sim_verdicts.get(&shot)
    }

    /// How many armour-piercing shells the comparison could not answer for.
    pub(crate) fn sim_unchecked(&self) -> usize {
        self.sim_unchecked
    }

    /// Sends each landed shell through this hull and compares the result with
    /// what the server said the shell did.
    ///
    /// The reading beside each shell in the incoming-fire log, which is how a
    /// disagreement between this app's ballistics and the game's is found. The
    /// egui viewer runs the same comparison as it draws each trajectory
    /// (`replay/realtime_armor_viewer.rs`); here it is run over the log rather
    /// than over what is drawn, so a shell need not be picked to be read.
    pub(crate) fn compare_incoming(
        &mut self,
        taken: &[PreExtractedHit],
        shells: &HashMap<GameParamId, ShellInfo>,
        cx: &mut Context<Self>,
    ) {
        let had = !self.sim_verdicts.is_empty() || self.sim_unchecked > 0;
        self.sim_verdicts.clear();
        self.sim_unchecked = 0;
        if let Some(armor) = self.current_armor.clone() {
            let center = armor.center();
            for taken in taken {
                let hit = &taken.hit;
                let Some(shell) = hit.salvo.as_ref().and_then(|salvo| shells.get(&salvo.params_id)) else { continue };
                // Only armour-piercing: the plate chain simulates AP, and
                // calling an HE hit a mismatch would report on the simulation
                // rather than on the shell.
                if shell.ammo_type != AmmoType::AP {
                    continue;
                }
                match self.compare_one(hit, shell, center) {
                    Some(verdict) => {
                        self.sim_verdicts.insert(hit.hit.shot_id, super::analysis::agreement(&verdict));
                    }
                    // A shell whose victim's pose was not held, whose flight
                    // does not solve, whose ray crosses no plating, or whose
                    // outcome the simulation does not model.
                    None => self.sim_unchecked += 1,
                }
            }
        }
        if had || !self.sim_verdicts.is_empty() || self.sim_unchecked > 0 {
            cx.notify();
        }
    }

    /// One landed shell, cast back along the way it came.
    fn compare_one(&self, hit: &ResolvedShotHit, shell: &ShellInfo, center: Vec3) -> Option<ComparisonVerdict> {
        let arrival = wows_replay_insights::hull_impact::shell_arrival(hit)?;
        // Placed without the hull's bounds, unlike a drawn marker: holding an
        // impact inside them keeps a marker off open water, but a strike dragged
        // to the hull's edge would be simulated through plating the shell never
        // met and reported as a disagreement.
        let at = wows_replay_insights::hull_impact::hull_impact(hit, [center.x, center.y, center.z], None)?;

        let params = ShellParams::from_shell_info(shell)?;
        // The arrival state comes from solving the shot's own flight: the
        // terminal-ballistics velocity a hit carries is post-impact.
        let impact = solve_for_range(&params, arrival.travelled)?;
        let fall = impact.impact_angle_horizontal;
        let (cos, sin) = (fall.cos(), fall.sin());
        let bearing = arrival.bearing;
        let shell_dir = Vec3::new(bearing.x * cos, -sin, bearing.z * cos).normalize();

        // Cast from outside the hull along that bearing, so the ray crosses
        // every plate the shell would have met rather than only those past the
        // point it stopped at. Through the impact's own height, since a hit up a
        // superstructure did not cross the belt.
        let through = Vec3::new(at[0], at[1], at[2]);
        let ray_hits = self.viewport.pick_all_ray_model_space(through - shell_dir * CAST_STANDOFF, shell_dir);
        let hits = trajectory::build_hits(&ray_hits, &self.mesh_triangle_info, &shell_dir);
        let first = hits.first()?;
        let strike = Strike { angle: first.angle_from_normal, thickness: first.thickness };
        let plates: Vec<ArcPlate> = hits
            .iter()
            .map(|hit| ArcPlate { zone: hit.zone.clone(), thickness: hit.thickness, material: hit.material })
            .collect();

        let sim = trajectory::simulate(&params, &impact, &hits, self.continue_on_ricochet);
        let server = ServerOutcome::from_shell_hit_type(&hit.hit.hit_type.shell_hit);
        compare_with_server(&sim, &plates, strike, &server, &params)
    }

    /// How many hits are drawn on the hull.
    pub(crate) fn hits_drawn(&self) -> usize {
        self.hits.len()
    }

    /// Says what the ship's health was at the moment being shown.
    ///
    /// Set apart from the hits because it changes without them: a fire or a
    /// flood takes health off between one shell and the next.
    pub(crate) fn set_hit_health(&mut self, health: Option<f32>, cx: &mut Context<Self>) {
        if self.hit_health == health {
            return;
        }
        self.hit_health = health;
        cx.notify();
    }

    /// The shot log beside the hull: what landed, newest first.
    ///
    /// Bounded, because a battleship's worth of secondaries is hundreds of
    /// lines and a log that long is not read, it is scrolled past.
    pub(crate) fn shot_log(&self) -> Vec<wows_replay_insights::hull_impact::LoggedHit> {
        self.hits.iter().rev().take(SHOT_LOG_LINES).map(wows_replay_insights::hull_impact::log_line).collect()
    }

    /// What a replay viewer reads beside the hull: how much of the ship was
    /// left, and how much of it had been hit.
    pub(crate) fn hit_readout(&self) -> Option<String> {
        let health = self.hit_health?;
        Some(t!("ui.armor.realtime_readout", health = format!("{health:.0}"), hits = self.hits.len()).into_owned())
    }

    /// The shown model's overall length and beam, while the length marks are.
    pub(crate) fn length_readout(&self) -> Option<String> {
        let marks = self.length_marks.as_ref().filter(|_| self.display_settings.show_length_marks)?.marks();
        let (length, beam) = (Reading::of_size(marks.length()).text(), Reading::of_size(marks.beam()).text());

        Some(t!("ui.armor.length_readout", length = length, beam = beam).into_owned())
    }

    /// Whether the openings in the armor are marked, and how many there
    /// were.
    pub(crate) fn gaps(&self) -> GapState {
        GapState { shown: self.show_gaps, count: self.gap_count }
    }

    /// How many shells have been cast, and whether a ricochet is followed.
    pub(crate) fn trajectory_state(&self) -> TrajectoryState {
        TrajectoryState { count: self.arcs.len(), continue_on_ricochet: self.continue_on_ricochet }
    }

    /// Adopts the ship and shell the next cast is fired with.
    ///
    /// An arc cast before any ship was chosen takes it, since it was waiting
    /// for one; an arc already fired with a shell keeps its own.
    pub(crate) fn set_cast_shell(&mut self, shell: Option<CastShell>, cx: &mut Context<Self>) {
        let unchanged = match (&self.cast_shell, &shell) {
            (Some(held), Some(next)) => held.is_same(next),
            (None, None) => true,
            _ => false,
        };
        if unchanged {
            return;
        }

        self.cast_shell = shell;
        if let Some(shell) = self.cast_shell.clone() {
            let waiting: Vec<ArcId> = self.arcs.iter().filter(|arc| arc.shell.is_none()).map(|arc| arc.id).collect();
            for id in waiting {
                self.set_arc_shell(id, shell.clone(), cx);
            }
        }
        cx.notify();
    }

    /// Sets the range the next cast is fired from. Arcs already cast keep
    /// their own; each is edited from its marker.
    pub(crate) fn set_cast_range(&mut self, range: Km, cx: &mut Context<Self>) {
        self.cast_range = range;
        cx.notify();
    }

    /// Whether a click bursts a shell, whether this ship has boxes to burst
    /// against, and what the last burst reached.
    pub(crate) fn splash_state(&self) -> SplashState {
        SplashState {
            shown: self.splash_mode,
            boxes_shown: self.show_splash_boxes,
            has_data: self.current_armor.as_ref().is_some_and(|armor| armor.splash_data.is_some()),
            zones_reached: self.splash_result.as_ref().map(|result| result.hit_zones.len()).unwrap_or(0),
        }
    }

    /// Turns splash bursting on or off.
    pub(crate) fn set_splash_mode(&mut self, on: bool, cx: &mut Context<Self>) {
        if self.splash_mode == on {
            return;
        }
        self.splash_mode = on;
        cx.notify();
    }

    /// Shows or hides the outlines of the ship's named splash boxes.
    pub(crate) fn set_show_splash_boxes(&mut self, show: bool, cx: &mut Context<Self>) {
        if self.show_splash_boxes == show {
            return;
        }
        self.show_splash_boxes = show;
        self.reupload_current_armor(cx);
        cx.notify();
    }

    /// The zones the burst on screen reached, for the list beside it.
    pub(crate) fn splash_zones(&self) -> &[splash::SplashZoneHit] {
        self.splash_result.as_ref().map(|result| result.hit_zones.as_slice()).unwrap_or(&[])
    }

    /// Bursts a shell where the pointer is.
    ///
    /// The burst is sized by the largest high-explosive or semi-armour-piercing
    /// shell the chosen attacker carries, which is the shell worth asking
    /// about: an armour-piercing round that fails to penetrate does not burst
    /// outside the hull. Clicking past the ship clears the burst.
    fn burst_at(&mut self, pointer: Vec2, cx: &mut Context<Self>) {
        let Some(bounds) = self.last_bounds else { return };
        let rect = view_rect_from_bounds(bounds);
        let Some(surface) = self.viewport.pick(pointer, rect) else {
            self.splash_result = None;
            self.reupload_current_armor(cx);
            cx.notify();
            return;
        };

        let Some(armor) = self.current_armor.clone() else { return };
        let Some(data) = armor.splash_data.as_ref() else { return };

        let point: [f32; 3] = surface.world_position.into();
        let reach = self.burst_reach();
        self.splash_result = Some(splash::compute_splash(point, reach, data, armor.hit_locations.as_ref()));
        self.reupload_current_armor(cx);
        cx.notify();
    }

    /// How far the chosen attacker's burst reaches.
    ///
    /// A ship with no bursting shell, or none chosen, still places a marker
    /// at the smallest reach rather than none, so the click is not silently
    /// ignored; the list beside it says no shell was found.
    fn burst_reach(&self) -> splash::ModelUnit {
        let largest = self.cast_shell.as_ref().map(|cast| &cast.ship).and_then(|ship| {
            ship.shells
                .iter()
                .filter(|shell| matches!(shell.ammo_type, AmmoType::HE | AmmoType::SAP))
                .map(|shell| shell.caliber)
                .max_by(|a, b| a.value().total_cmp(&b.value()))
        });
        match largest {
            Some(caliber) => splash::splash_half_extent(caliber),
            None => splash::ModelUnit::new(0.0),
        }
    }

    /// Whether a ricocheting shell is followed past the bounce.
    ///
    /// Everything already cast is fired again, since the answer changes where
    /// each of those shells ended.
    pub(crate) fn set_continue_on_ricochet(&mut self, on: bool, cx: &mut Context<Self>) {
        if self.continue_on_ricochet == on {
            return;
        }
        self.continue_on_ricochet = on;
        let refired: Vec<trajectory::Trajectory> =
            self.arcs.iter().map(|arc| self.refire(arc, arc.trajectory.range, arc.shell.as_ref())).collect();
        for (arc, trajectory) in self.arcs.iter_mut().zip(refired) {
            arc.set_trajectory(trajectory);
        }
        cx.notify();
    }

    /// Drops every cast shell.
    pub(crate) fn clear_trajectories(&mut self, cx: &mut Context<Self>) {
        if self.arcs.is_empty() {
            return;
        }
        let was_isolating =
            self.arcs.iter().any(|arc| arc.trajectory.isolating_plates || arc.trajectory.isolating_zones);
        self.arcs.clear();
        self.selected_arc = None;
        // What the arcs were hiding is given back with them, rather than
        // leaving most of the ship gone with nothing on screen saying why.
        if was_isolating {
            self.apply_arc_isolation(cx);
            return;
        }
        cx.notify();
    }

    /// Marks the openings in the armor, or stops marking them.
    pub(crate) fn set_show_gaps(&mut self, show: bool, cx: &mut Context<Self>) {
        if self.show_gaps == show {
            return;
        }
        self.show_gaps = show;
        self.reupload_current_armor(cx);
        cx.notify();
    }

    /// Whether only the plates the game's own viewer hides are being shown.
    pub(crate) fn show_hidden_only(&self) -> bool {
        self.show_hidden_only
    }

    /// Shows only the plates the game's own viewer hides, or everything
    /// again. Changes which triangles are uploaded, so the armor is rebuilt.
    pub(crate) fn set_show_hidden_only(&mut self, hidden_only: bool, cx: &mut Context<Self>) {
        if self.show_hidden_only == hidden_only {
            return;
        }
        self.show_hidden_only = hidden_only;
        self.reupload_current_armor(cx);
        cx.notify();
    }

    pub(crate) fn has_armor(&self) -> bool {
        self.current_armor.is_some()
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
        self.last_plate.clone()
    }

    pub(crate) fn shown_ship_name(&self) -> Option<SharedString> {
        self.scene_source.as_ref().map(|source| SharedString::from(source.display_name().to_string()))
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

    /// Starts a load into this pane, naming it while it runs, and cancels
    /// the first stage of the one before. A result lands only while its
    /// generation is still the latest one this pane started, so a newer
    /// selection here supersedes it and one in another pane does not.
    pub(crate) fn begin_load(&mut self, name: SharedString, cx: &mut Context<Self>) -> OpeningLoad {
        self.load_generation = LoadGeneration(self.load_generation.0 + 1);
        let cancel = self.opening_load.start();
        self.scene_problem = None;
        self.set_ship_loading(Some(name), cx);

        OpeningLoad { generation: self.load_generation, cancel }
    }

    pub(crate) fn is_current_load(&self, generation: LoadGeneration) -> bool {
        self.load_generation == generation
    }

    /// Cancels the load in flight, if any, so it stops and its result is
    /// dropped. Whether one was in flight.
    pub(crate) fn cancel_load(&mut self, cx: &mut Context<Self>) -> bool {
        if self.ship_loading.is_none() {
            return false;
        }

        self.load_generation = LoadGeneration(self.load_generation.0 + 1);
        self.opening_load.cancel();
        self.set_ship_loading(None, cx);

        true
    }

    /// Runs the background stage of `scene`, which this viewport has just
    /// been given to show. Each job stores its piece in the scene as it
    /// lands, and every viewport of the pane showing the scene then takes it
    /// up. Cancelled once no viewport shows the scene.
    pub(crate) fn start_scene_rest(&mut self, scene: Arc<LoadedShipArmor>, rest: Vec<RestJob>, cx: &mut Context<Self>) {
        let run = SharedLoad::start();
        let cancel = run.cancel_flag().clone();
        self.rest_load = Some(run);
        for job in rest {
            let piece = job.piece();
            let job_scene = Arc::clone(&scene);
            let job_cancel = cancel.clone();
            let ran = cx.background_spawn(async move { job.run(&job_scene, &job_cancel) });
            let scene = Arc::clone(&scene);
            // Through the pane rather than this viewport, which may have
            // closed while another still shows the scene.
            let pane = self.pane.clone();
            cx.spawn(async move |this, cx| {
                match ran.await {
                    Ok(()) => {}
                    Err(RestJobError::Cancelled(_)) => return,
                    Err(e) => {
                        tracing::warn!("armor viewer: {} lacks its {piece:?}: {e}", scene.display_name);
                        return;
                    }
                }
                let viewports = cx.update(|cx| match pane.as_ref().and_then(WeakEntity::upgrade) {
                    Some(pane) => pane.read(cx).viewports(cx),
                    None => this.upgrade().into_iter().collect(),
                });
                for viewport in viewports {
                    viewport.update(cx, |view, cx| view.take_up_rest(&scene, piece, cx));
                }
            })
            .detach();
        }
    }

    /// The background stage of the scene shown, for a viewport that is
    /// given the same scene to hold too.
    pub(crate) fn scene_rest(&self) -> Option<Arc<SharedLoad>> {
        self.rest_load.clone()
    }

    /// Holds `run`, the background stage of the scene just shown here, which
    /// another viewport started.
    pub(crate) fn share_scene_rest(&mut self, run: Arc<SharedLoad>) {
        self.rest_load = Some(run);
    }

    /// Takes up `piece` of `scene`, when it is the scene shown.
    fn take_up_rest(&mut self, scene: &Arc<LoadedShipArmor>, piece: RestPiece, cx: &mut Context<Self>) {
        // A scene still waiting for the device reads every piece landed so
        // far once it is uploaded.
        if !self.current_armor.as_ref().is_some_and(|shown| Arc::ptr_eq(shown, scene)) {
            return;
        }

        match piece {
            // A pointer resting on the hull while the index was built hovers
            // it now, as a move there would.
            RestPiece::Pick => self.hover_again(),
            // Read by the next draw of the camo picker.
            RestPiece::Camo => {}
        }
        cx.notify();
    }

    /// Picks the hover again where the pointer last moved over the view.
    fn hover_again(&mut self) {
        let Some(at) = self.last_pointer else { return };
        let over_view = self.last_bounds.is_some_and(|bounds| bounds.contains(&at));
        if over_view && self.drag.is_none() && self.camera_is_free() {
            self.update_plate_hover(at);
        }
    }

    /// Shows `problem` in place of the scene. The scene stays loaded, so the
    /// selectors that led here can pick a level of detail or hull that draws.
    pub(crate) fn show_problem(&mut self, problem: SceneProblem, cx: &mut Context<Self>) {
        self.scene_problem = Some(problem);
        cx.notify();
    }

    /// Drops the scene and shows `problem` in its place. The failed load was
    /// for another subject, which the old scene must not pass for: the
    /// sidebar and option rail would go on describing it.
    pub(crate) fn fail_load(&mut self, problem: SceneProblem, cx: &mut Context<Self>) {
        self.clear_scene();
        self.show_problem(problem, cx);
    }

    pub(crate) fn scene_problem(&self) -> Option<&SceneProblem> {
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

    /// Paints the scene at `bounds`, drawing it again first if it changed or
    /// the target no longer matches the painted size in device pixels. Runs
    /// during paint, so the frame being drawn samples the new contents.
    fn paint_scene(&mut self, bounds: Bounds<Pixels>, window: &mut Window) {
        // The markers' least on-screen size is held in scene units, so a zoom
        // or resize that moves it far enough builds them again.
        if self.hardpoint_mesh.is_some() {
            let world_per_pixel = self.viewport.camera.world_per_pixel(bounds.size.height.as_f32());
            let ratio = world_per_pixel / self.hardpoint_world_per_pixel.max(f32::MIN_POSITIVE);
            if !(0.8..=1.25).contains(&ratio) {
                self.build_hardpoints(world_per_pixel);
            }
        }
        let Some(scene) = self.scene() else { return };
        let scale = window.scale_factor();
        let Some(size) = TargetSize::new(
            (bounds.size.width.as_f32() * scale).round() as u32,
            (bounds.size.height.as_f32() * scale).round() as u32,
        ) else {
            return;
        };

        match scene.paint_view(self.entity_id, &self.viewport, self.viewport.is_dirty(), size, window, bounds) {
            Ok(true) => self.viewport.clear_dirty(),
            Ok(false) => {}
            Err(error) => tracing::warn!("armor viewport: {error}"),
        }
    }

    /// The names of the splash boxes and where each is written, while the
    /// boxes are outlined.
    pub(crate) fn splash_box_labels(&self) -> &[splash::SplashBoxLabel] {
        &self.splash_labels
    }

    /// Draws what is laid over the scene in screen space: the splash box
    /// names, the length marks' distances and the cast arcs. In screen space,
    /// so a label or a line keeps its size however far the camera pulls back.
    fn paint_annotations(&self, bounds: Bounds<Pixels>, window: &mut Window, cx: &mut App) {
        let rect = view_rect_from_bounds(bounds);
        let camera = &self.viewport.camera;
        let on_screen = |at: Vec2| point(px(at.x), px(at.y));

        for label in self.splash_box_labels() {
            let anchor = self.viewport.pos_to_world_space(Vec3::from(label.position));
            let Some(at) = camera.project_to_screen(anchor, rect).filter(|at| rect.contains(*at)) else { continue };
            paint_box_label(&label.name, on_screen(at), 1., window, cx);
        }

        // The grid is drawn in world space, so its labels are placed there too.
        if let Some(written) = self.length_marks.as_ref().filter(|_| self.display_settings.show_length_marks) {
            let eye = camera.eye_position();
            let marks = written.marks();
            let labels = written
                .labels(|a, b| (a - eye).norm() < (b - eye).norm(), |position| depth_cues::grid_fade(marks, position));
            for label in labels {
                let Some(at) = camera.project_to_screen(label.position, rect).filter(|at| rect.contains(*at)) else {
                    continue;
                };
                paint_box_label(label.text, on_screen(at), label.opacity, window, cx);
            }
        }

        // A few markers are named outright; past that the names would bury
        // the model, so only the one under the pointer is.
        const LABELLED_OUTRIGHT: usize = 16;
        let hardpoint_labels: Vec<&hardpoint_overlay::HardpointLabel> =
            if self.hardpoint_labels.len() <= LABELLED_OUTRIGHT {
                self.hardpoint_labels.iter().collect()
            } else {
                self.hovered_hardpoint.and_then(|index| self.hardpoint_labels.get(index)).into_iter().collect()
            };
        for label in hardpoint_labels {
            let anchor = self.viewport.pos_to_world_space(Vec3::from(label.position));
            let Some(at) = camera.project_to_screen(anchor, rect).filter(|at| rect.contains(*at)) else { continue };
            paint_box_label(&label.name, on_screen(at), 1., window, cx);
        }

        // The arc being edited is drawn last, over the others.
        let selected = self.selected_arc.as_ref().map(|selected| selected.id);
        let others = self.arcs.iter().filter(|arc| Some(arc.id) != selected);
        let editing = self.arcs.iter().filter(|arc| Some(arc.id) == selected);
        for arc in others.chain(editing) {
            paint_arc(
                &arc.drawing,
                arc.color.rgba(),
                Some(arc.id) == selected,
                |at| camera.project_to_screen(at, rect).map(on_screen),
                window,
            );
        }
    }

    /// Writes the length marks' distances again when the language has
    /// changed since they were written.
    fn rewrite_length_marks(&mut self) {
        if let Some(written) = self.length_marks.as_ref().filter(|written| !written.is_current()) {
            self.length_marks = Some(MarkLabels::new(*written.marks()));
        }
    }

    /// Where the navigation gizmo sits, or `None` while there is no ship for
    /// it to orient -- it is not drawn then, so nothing may be aimed at it
    /// either.
    fn gizmo_rect(&self) -> Option<ViewRect> {
        self.current_armor.as_ref()?;
        // Nothing to snap while the lock owns the camera, so the box is not
        // there to be hit either.
        self.camera_is_free().then_some(())?;
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
                if !in_gizmo && event.click_count >= 2 && !self.perspective.enabled {
                    if let Some(home) = self.home_camera.clone() {
                        self.viewport.camera = home;
                        self.viewport.mark_dirty();
                        cx.emit(ViewportChange::Camera);
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
        self.last_pointer = Some(event.position);
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
                // A locked camera is steered on its orbit instead: orbiting the
                // model would fight the lock, which puts the camera back every
                // upload.
                if self.perspective.enabled {
                    if self.steer_perspective(dx, dy) {
                        self.refresh_perspective(cx);
                    }
                } else {
                    match drag.kind {
                        DragKind::Orbit => self.viewport.camera.orbit((dx, dy), size),
                        DragKind::Pan => self.viewport.camera.pan((dx, dy), size),
                    }
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
            cx.emit(ViewportChange::Camera);
        }

        // Plate picking only runs while not dragging the camera/gizmo, so it
        // doesn't fight with (or waste CPU during) an orbit/pan; any hover
        // from before the drag started is cleared so the tooltip/highlight
        // don't sit stale over a moving model. It is off entirely while the
        // camera is locked, which is a view rather than a place to edit from,
        // as the egui viewer suppresses it (`ui/tab.rs`).
        let plate_hover_changed = if self.drag.is_none() && self.camera_is_free() {
            let ring_changed = self.update_ring_hover(event.position);
            let hardpoint_changed = self.update_hardpoint_hover(event.position);
            self.update_plate_hover(event.position) || ring_changed || hardpoint_changed
        } else {
            let ring_changed = self.clear_ring_hover();
            let hardpoint_changed = self.hovered_hardpoint.take().is_some();
            let plate_changed = self.clear_plate_hover();
            self.clear_mesh_hover() || plate_changed || ring_changed || hardpoint_changed
        };

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
        let pointer = point_to_vec2(event.position);
        let action =
            click_action(event.button, plain_click, self.gizmo_press_in_box, self.splash_mode, event.modifiers.shift);
        match action {
            ClickAction::SnapGizmo => {
                if let Some(rect) = self.gizmo_rect()
                    && let Some((axis, positive)) = gizmo::hit_test(rect, &self.viewport.camera, pointer)
                {
                    self.snap_camera(axis, positive, cx);
                }
            }
            ClickAction::Cast => self.cast_at(pointer, cx),
            ClickAction::Burst => self.burst_at(pointer, cx),
            ClickAction::TogglePlate => {
                // A plain click on the model, outside the gizmo box, toggles
                // the hovered plate's visibility, matching the egui app's
                // `response.clicked()` click-to-hide (`tab.rs:5350-5362`).
                if self.camera_is_free()
                    && let Some(key) = self.hovered.as_ref().map(|h| h.key.clone())
                {
                    self.toggle_plate(key, cx);
                }
            }
            ClickAction::Nothing => {}
        }
        self.gizmo_press_in_box = false;
    }

    /// Casts a shell at whatever the pointer is over, adds it to the arcs
    /// already cast, and opens its editor. A cast past the ship does nothing.
    ///
    /// The shell comes in along the camera's own bearing, flattened: a
    /// trajectory is read against the ship, and firing from wherever the eye
    /// happens to sit vertically would say more about the camera than the
    /// armor.
    fn cast_at(&mut self, pointer: Vec2, cx: &mut Context<Self>) {
        let Some(bounds) = self.last_bounds else { return };
        let rect = view_rect_from_bounds(bounds);
        let fired = self.viewport.pick(pointer, rect).and_then(|surface| {
            let (_, ray) = self.viewport.screen_to_ray(pointer, rect)?;
            let approach = trajectory::approach_xz(&ray);
            Some((approach, self.fire_at(surface.world_position, approach)?))
        });
        let Some((approach, trajectory)) = fired else { return };

        self.add_arc(approach, trajectory, cx);
    }

    /// Keeps a cast shell, fired with the next cast's ship and shell, in a
    /// colour no other arc holds, and opens its editor: the arc just cast is
    /// the one the reader means to look at.
    pub(super) fn add_arc(
        &mut self,
        approach: Vec3,
        trajectory: trajectory::Trajectory,
        cx: &mut Context<Self>,
    ) -> ArcId {
        let id = self.next_arc;
        self.next_arc = id.next();
        let color = ArcColor::unused(self.arcs.iter().map(|arc| arc.color));
        let drawing = trajectory::drawing(&trajectory);
        self.arcs.push(CastArc { id, color, shell: self.cast_shell.clone(), approach, trajectory, drawing });
        self.selected_arc = self.arc_editor(id, cx);
        cx.notify();

        id
    }

    /// A shell fired with the next cast's settings at `aim`, coming in on
    /// `approach`. `None` when the ray crosses no plate anything is known
    /// about.
    fn fire_at(&self, aim: Vec3, approach: Vec3) -> Option<trajectory::Trajectory> {
        let fired = self.cast_shell.as_ref().and_then(|shell| shell.fire(self.cast_range));
        let shell_dir = fall_direction(approach, fired.as_ref().map(|(_, impact)| impact));

        // Cast from outside the hull so the ray crosses every plate on the
        // way in, not just those past the point clicked.
        let origin = aim - shell_dir * CAST_STANDOFF;
        let ray_hits = self.viewport.pick_all_ray(origin, shell_dir);
        let hits = trajectory::build_hits(&ray_hits, &self.mesh_triangle_info, &shell_dir);
        if hits.is_empty() {
            return None;
        }

        let shell = fired.as_ref().map(|(params, impact)| (params, impact));
        Some(trajectory::cast(hits, shell_dir, shell, self.model_extent(), self.continue_on_ricochet, self.cast_range))
    }

    /// `arc` fired again from `range` with `shell`, at the plates it first
    /// crossed, keeping what it isolates.
    fn refire(&self, arc: &CastArc, range: Km, shell: Option<&CastShell>) -> trajectory::Trajectory {
        let fired = shell.and_then(|shell| shell.fire(range));
        let shell_dir = fall_direction(arc.approach, fired.as_ref().map(|(_, impact)| impact));
        let hits = trajectory::reaim(arc.trajectory.hits.clone(), &shell_dir);
        let shell = fired.as_ref().map(|(params, impact)| (params, impact));
        let mut refired =
            trajectory::cast(hits, shell_dir, shell, self.model_extent(), self.continue_on_ricochet, range);
        refired.isolating_plates = arc.trajectory.isolating_plates;
        refired.isolating_zones = arc.trajectory.isolating_zones;
        refired
    }

    /// Every cast arc, as the trajectory section lists them.
    pub(crate) fn arcs(&self) -> Vec<ArcSummary> {
        self.arcs
            .iter()
            .map(|arc| {
                let cast = &arc.trajectory;
                let plates: Vec<ArcPlate> = cast
                    .hits
                    .iter()
                    .map(|hit| ArcPlate { zone: hit.zone.clone(), thickness: hit.thickness, material: hit.material })
                    .collect();
                ArcSummary {
                    id: arc.id,
                    color: arc.color,
                    hits: cast.hits.len(),
                    total_armor: Millimeters::from(cast.hits.iter().map(|hit| hit.thickness.value()).sum::<f32>()),
                    range: cast.range,
                    shell: arc.shell.clone(),
                    isolating_plates: cast.isolating_plates,
                    isolating_zones: cast.isolating_zones,
                    outcome: describe_arc(cast.sim.as_ref(), &plates),
                }
            })
            .collect()
    }

    /// The arc being edited, and the slider its range is edited with.
    pub(crate) fn selected_arc(&self) -> Option<(ArcId, Entity<SliderField>)> {
        self.selected_arc.as_ref().map(|selected| (selected.id, selected.range.clone()))
    }

    /// Opens the editor on an arc, or puts it away when that arc is the one
    /// open.
    pub(crate) fn toggle_arc_selection(&mut self, id: ArcId, cx: &mut Context<Self>) {
        let open = self.selected_arc.as_ref().is_some_and(|selected| selected.id == id);
        self.selected_arc = if open { None } else { self.arc_editor(id, cx) };
        cx.notify();
    }

    /// An editor on the arc `id`. `None` when no such arc is cast.
    fn arc_editor(&mut self, id: ArcId, cx: &mut Context<Self>) -> Option<SelectedArc> {
        let range = self.arcs.iter().find(|arc| arc.id == id)?.trajectory.range;
        let slider = Self::new_slider(cx, CAST_RANGE_SLIDER, range.value(), DEFAULT_CAST_RANGE.value());
        let changed = Self::subscribe_slider(cx, &slider, move |this, value, cx| {
            this.set_arc_range(id, Km::new(value), cx);
        });

        Some(SelectedArc { id, range: slider, _range_changed: changed })
    }

    /// Drops one arc.
    pub(crate) fn remove_arc(&mut self, id: ArcId, cx: &mut Context<Self>) {
        let Some(index) = self.arcs.iter().position(|arc| arc.id == id) else { return };
        let dropped = self.arcs.remove(index);
        if self.selected_arc.as_ref().is_some_and(|selected| selected.id == id) {
            self.selected_arc = None;
        }

        // Only what it was hiding has to be given back; an arc that hid nothing
        // leaves the reader's own hiding where it is.
        if dropped.trajectory.isolating_plates || dropped.trajectory.isolating_zones {
            self.apply_arc_isolation(cx);
            return;
        }
        cx.notify();
    }

    /// Fires one arc from a different range, leaving the others where they
    /// are. The plates it crossed stay the same, so what it isolates does too.
    pub(crate) fn set_arc_range(&mut self, id: ArcId, range: Km, cx: &mut Context<Self>) {
        let Some(index) = self.arcs.iter().position(|arc| arc.id == id) else { return };
        let held = &self.arcs[index];
        if (held.trajectory.range.value() - range.value()).abs() < f32::EPSILON {
            return;
        }

        let refired = self.refire(held, range, held.shell.as_ref());
        self.arcs[index].set_trajectory(refired);
        cx.notify();
    }

    /// Fires one arc with a different ship's shell, or another of its own.
    pub(crate) fn set_arc_shell(&mut self, id: ArcId, shell: CastShell, cx: &mut Context<Self>) {
        let Some(index) = self.arcs.iter().position(|arc| arc.id == id) else { return };
        let held = &self.arcs[index];

        let refired = self.refire(held, held.trajectory.range, Some(&shell));
        self.arcs[index].set_trajectory(refired);
        self.arcs[index].shell = Some(shell);
        cx.notify();
    }

    /// Hides what one arc did not cross, or stops hiding it.
    ///
    /// Plates and zones are two answers to one question, so asking for either
    /// withdraws the other.
    pub(crate) fn isolate_arc(&mut self, id: ArcId, what: Isolate, on: bool, cx: &mut Context<Self>) {
        let Some(arc) = self.arcs.iter_mut().find(|arc| arc.id == id) else { return };
        let cast = &mut arc.trajectory;
        match what {
            Isolate::Plates => {
                cast.isolating_plates = on;
                cast.isolating_zones = false;
            }
            Isolate::Zones => {
                cast.isolating_zones = on;
                cast.isolating_plates = false;
            }
        }
        self.apply_arc_isolation(cx);
    }

    /// The same for every arc at once, which is what reads out the whole cast.
    pub(crate) fn isolate_all_arcs(&mut self, what: Isolate, cx: &mut Context<Self>) {
        for arc in &mut self.arcs {
            arc.trajectory.isolating_plates = what == Isolate::Plates;
            arc.trajectory.isolating_zones = what == Isolate::Zones;
        }
        self.apply_arc_isolation(cx);
    }

    /// Rebuilds the visibility the arcs' isolation asks for.
    ///
    /// The whole visibility is recomputed rather than adjusted, because an arc
    /// that stops isolating has to give back exactly what it hid and nothing
    /// else. With no arc isolating anything, the ship goes back to whole.
    fn apply_arc_isolation(&mut self, cx: &mut Context<Self>) {
        let mut hit_zones: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut hit_plates: std::collections::HashSet<PlateKeyParts> = std::collections::HashSet::new();
        for cast in self.arcs.iter().map(|arc| &arc.trajectory) {
            for hit in &cast.hits {
                if cast.isolating_zones {
                    hit_zones.insert(hit.zone.clone());
                }
                if cast.isolating_plates {
                    hit_plates.insert((
                        hit.zone.clone(),
                        hit.material.name().to_owned(),
                        (hit.thickness.value() * 10.0).round() as i32,
                    ));
                }
            }
        }

        self.undo_stack.push(VisibilitySnapshot {
            part_visibility: self.part_visibility.clone(),
            plate_visibility: self.plate_visibility.clone(),
        });
        self.part_visibility.clear();
        self.mesh_seed.armor = false;
        self.plate_visibility.clear();
        if let Some(armor) = self.current_armor.clone() {
            let zones: Vec<ZoneShape<'_>> = armor
                .zone_part_plates
                .iter()
                .map(|zone| ZoneShape {
                    name: zone.name.as_str(),
                    parts: zone.parts.iter().map(|part| (part.name.as_str(), part.plates.as_slice())).collect(),
                })
                .collect();
            let isolation = isolate(&zones, &hit_zones, &hit_plates);
            self.part_visibility = isolation.parts;
            // The shared map says whether a plate is shown; this port's says
            // whether it is hidden (`visibility.rs`), so the sense is turned
            // over on the way in.
            self.plate_visibility = isolation
                .plates
                .into_iter()
                .map(|((zone, material_name, thickness_tenths), shown)| {
                    (PlateKey { zone, material_name, thickness_tenths }, !shown)
                })
                .collect();
        }
        self.reupload_current_armor(cx);
        cx.notify();
    }

    /// How wide the ship is, which sets how long an incoming arc is drawn.
    ///
    /// A ship whose bounds are not known yet gets a default rather than a
    /// zero-length arc.
    fn model_extent(&self) -> f32 {
        match self.model_bounds {
            Some((min, max)) => (max[0] - min[0]).max(max[2] - min[2]),
            None => DEFAULT_MODEL_EXTENT,
        }
    }

    fn handle_mouse_up_out(&mut self, _event: &MouseUpEvent, _window: &mut Window, cx: &mut Context<Self>) {
        self.gizmo_press_in_box = false;
        if self.drag.take().is_some() {
            cx.notify();
        }
    }

    fn handle_scroll_wheel(&mut self, event: &ScrollEvent, window: &mut Window, cx: &mut Context<Self>) {
        if !self.is_gpu_ready() {
            return;
        }
        let delta = event.delta.pixel_delta(window.line_height());
        let dy = delta.y.as_f32();
        if dy != 0.0 {
            // The wheel moves the locked camera between the inner and outer
            // orbit rather than pulling it back off them.
            if self.perspective.enabled {
                if self.zoom_perspective(dy) {
                    self.refresh_perspective(cx);
                }
            } else {
                self.viewport.camera.zoom(dy);
            }
            self.viewport.mark_dirty();
            cx.emit(ViewportChange::Camera);
            cx.notify();
        }
    }

    /// Ctrl/Cmd+S opens and closes the options rail. Ctrl/Cmd+Z
    /// (no shift) undoes; Ctrl/Cmd+Shift+Z or Ctrl/Cmd+R redoes.
    /// Otherwise WASD moves the camera target, arrows move it vertically /
    /// orbit the azimuth and Shift+arrows pan it, tracking the key as held and (re)starting the
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
            if modifiers.secondary(cx) && key == "s" {
                if let Some(pane) = self.pane() {
                    pane.update(cx, |pane, cx| pane.toggle_options(cx));
                }
                return;
            }
            if modifiers.secondary(cx) && !modifiers.shift && key == "z" {
                self.undo_visibility(cx);
                return;
            }
            if modifiers.secondary(cx) && (key == "r" || (key == "z" && modifiers.shift)) {
                self.redo_visibility(cx);
                return;
            }
        }
        let shift = event.keystroke.modifiers.shift;
        let Some(key) = MoveKey::from_key(event.keystroke.key.as_str(), shift) else { return };
        if self.held_keys.insert(key) {
            self.start_key_ticker(cx);
        }
    }

    /// Removes a released movement key from the held set. The ticker itself
    /// notices an empty set and stops (no idle cost once nothing is held).
    fn handle_key_up(&mut self, event: &KeyUpEvent, _window: &mut Window, _cx: &mut Context<Self>) {
        for key in MoveKey::released_by(event.keystroke.key.as_str()) {
            self.held_keys.remove(&key);
        }
    }

    /// Applies one tick's worth of camera movement for every currently held
    /// movement key, mirroring the egui original's per-frame `key_down` poll
    /// (`viewport_3d/camera.rs` `handle_input`). `dt` is the time since the
    /// previous tick, which the pan is scaled by.
    fn apply_held_keys(&mut self, dt: Duration) -> bool {
        if self.held_keys.is_empty() || !self.camera_is_free() {
            return false;
        }

        let motion = HeldMotion::from_keys(&self.held_keys);
        if motion.forward != 0.0 || motion.right != 0.0 {
            self.viewport.camera.wasd(motion.forward, motion.right);
        }
        if motion.vertical != 0.0 {
            self.viewport.camera.move_vertical(motion.vertical);
        }
        if motion.rotate != 0.0 {
            self.viewport.camera.rotate_horizontal(motion.rotate);
        }
        if motion.pan_x != 0.0 || motion.pan_y != 0.0 {
            let size = self.viewport_size();
            self.viewport.camera.pan_keys((motion.pan_x, motion.pan_y), dt.as_secs_f32(), size);
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
            let mut last_tick = Instant::now();
            loop {
                cx.background_executor().timer(Duration::from_secs_f32(1.0 / KEY_TICK_HZ)).await;
                let now = Instant::now();
                let dt = now - last_tick;
                last_tick = now;
                let still_held = this.update(cx, |this, cx| {
                    let moved = this.apply_held_keys(dt);
                    if moved {
                        this.viewport.mark_dirty();
                        cx.emit(ViewportChange::Camera);
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
        self.scene().is_some()
    }

    /// CPU-picks at `position` (screen space) and updates `self.hovered` or,
    /// when no plate is hit, `self.hovered_mesh` to match, as the egui app's
    /// own per-frame hover pick does for plates (`tab.rs:5326-5347`).
    /// Rebuilds a highlight overlay mesh only when the hovered plate or mesh
    /// name actually changed (an on-demand GPU upload); the cursor position
    /// is still refreshed on every call so the floating tooltip element
    /// tracks the pointer. Returns whether the caller should `cx.notify()`.
    fn update_plate_hover(&mut self, position: Point<Pixels>) -> bool {
        let Some(bounds) = self.last_bounds else { return self.clear_plate_hover() | self.clear_mesh_hover() };
        let Some(armor) = self.current_armor.clone() else {
            return self.clear_plate_hover() | self.clear_mesh_hover();
        };
        let rect = view_rect_from_bounds(bounds);
        let pointer = point_to_vec2(position);
        let hit = self.viewport.pick(pointer, rect);
        let plate =
            hit.as_ref().and_then(|h| picking_ui::tooltip_for_hit(h, &self.mesh_triangle_info)).map(|tooltip| {
                let key = picking_ui::plate_key_of(tooltip);
                (key, tooltip.clone())
            });
        let (key, tooltip) = match HoverTarget::resolve(plate, || self.pick_mesh(&armor, pointer, rect)) {
            HoverTarget::Plate(plate) => plate,
            HoverTarget::Mesh(hit) => {
                self.clear_plate_hover();
                return self.set_mesh_hover(&armor, hit, position);
            }
            HoverTarget::Nothing => return self.clear_plate_hover() | self.clear_mesh_hover(),
        };
        self.clear_mesh_hover();

        let key_changed = self.hovered.as_ref().map(|h| h.key != key).unwrap_or(true);
        // Remembered past the hover itself: the penetration checker is opened
        // from a button, which the pointer has to leave the plate to reach.
        self.last_plate = Some((SharedString::from(tooltip.zone.clone()), tooltip.thickness_mm));
        self.hovered = Some(HoverInfo { key: key.clone(), tooltip, cursor: position });
        if key_changed {
            self.rebuild_hover_highlight(&key);
        }
        true
    }

    /// The nearest shown mesh along the ray through `pointer`.
    fn pick_mesh(&self, armor: &LoadedShipArmor, pointer: Vec2, rect: ViewRect) -> Option<MeshHit> {
        let (origin, direction) = self.viewport.screen_to_model_ray(pointer, rect)?;

        armor.hull_pick_index()?.pick(origin, direction, &self.hull_shown)
    }

    /// Hovers the mesh `hit` names, rebuilding the highlight only when the
    /// hovered mesh name changed. Returns whether the caller should
    /// `cx.notify()`.
    fn set_mesh_hover(&mut self, armor: &LoadedShipArmor, hit: MeshHit, cursor: Point<Pixels>) -> bool {
        let (Some(mesh), Some(source)) =
            (armor.hull_meshes.get(hit.mesh.get()), armor.hull_pick_index().and_then(|pick| pick.source(hit.mesh)))
        else {
            return self.clear_mesh_hover();
        };

        let tooltip = match self.hovered_mesh.take() {
            Some(hover) if hover.mesh == hit.mesh => hover.tooltip,
            _ => MeshTooltip::new(mesh, source),
        };
        self.hovered_mesh = Some(MeshHover { mesh: hit.mesh, tooltip, cursor });
        let highlighted = self.mesh_highlight.as_ref().map(|(name, _)| name.as_str());
        match HighlightChange::between(highlighted, Some(&mesh.name)) {
            HighlightChange::Keep => {}
            HighlightChange::Show(name) => self.show_mesh_highlight(name),
            HighlightChange::Remove => self.remove_mesh_highlight(),
        }

        true
    }

    /// Clears any hovered mesh, its tooltip and its highlight. Returns
    /// whether there was one.
    fn clear_mesh_hover(&mut self) -> bool {
        self.remove_mesh_highlight();

        self.hovered_mesh.take().is_some()
    }

    /// Replaces the mesh highlight with one over every mesh named `name`.
    fn show_mesh_highlight(&mut self, name: String) {
        self.remove_mesh_highlight();
        let Some(scene) = self.scene() else { return };
        let Some(armor) = self.current_armor.clone() else { return };
        let names = [name];
        let mesh_id = hull_picking::upload_hull_highlight(
            &mut self.viewport,
            scene.device(),
            &armor.hull_meshes,
            &names,
            picking_ui::HOVER_HIGHLIGHT_COLOR,
        );
        let [name] = names;
        self.mesh_highlight = Some((name, mesh_id));
        self.viewport.mark_dirty();
    }

    fn remove_mesh_highlight(&mut self) {
        if let Some((_, old_id)) = self.mesh_highlight.take() {
            self.viewport.remove_mesh(old_id);
            self.viewport.mark_dirty();
        }
    }

    /// Keeps the hovered mesh after its meshes were uploaded again: drops it
    /// once hidden, else draws its highlight again if the upload removed it.
    fn revalidate_mesh_hover(&mut self) {
        let Some(armor) = self.current_armor.clone() else { return };
        let shown = self
            .hovered_mesh
            .as_ref()
            .filter(|hover| self.hull_shown.get(hover.mesh.get()) == Some(&true))
            .and_then(|hover| armor.hull_meshes.get(hover.mesh.get()));
        match shown {
            Some(mesh) if self.mesh_highlight.is_none() => self.show_mesh_highlight(mesh.name.clone()),
            Some(_) => {}
            None => {
                self.clear_mesh_hover();
            }
        }
    }

    /// Reads the camera orbit under the pointer, if it is resting on one.
    ///
    /// Screen-space rather than a pick: an orbit is drawn a few centimetres
    /// wide, and a ray would miss it at every angle a ship is looked at
    /// from. Returns whether what it reads changed.
    fn update_ring_hover(&mut self, position: Point<Pixels>) -> bool {
        if self.ring_hovers.is_empty() {
            return self.clear_ring_hover();
        }
        let Some(bounds) = self.last_bounds else { return self.clear_ring_hover() };
        let rect = view_rect_from_bounds(bounds);
        let camera = &self.viewport.camera;
        let viewport = &self.viewport;
        let found = camera_rings::nearest_ring_label(
            &self.ring_hovers,
            |point| camera.project_to_screen(viewport.pos_to_world_space(point), rect).map(|at| [at.x, at.y]),
            [position.x.as_f32(), position.y.as_f32()],
        )
        .map(SharedString::from);

        let changed = found.as_ref() != self.hovered_ring.as_ref().map(|(label, _)| label);
        self.hovered_ring = found.map(|label| (label, position));
        changed
    }

    /// Forgets the orbit under the pointer. Returns whether there was one.
    fn clear_ring_hover(&mut self) -> bool {
        self.hovered_ring.take().is_some()
    }

    /// Reads the hardpoint marker nearest the pointer, within a few pixels.
    /// Returns whether it changed.
    fn update_hardpoint_hover(&mut self, position: Point<Pixels>) -> bool {
        const REACH_PX: f32 = 10.0;

        let found = self.last_bounds.and_then(|bounds| {
            let rect = view_rect_from_bounds(bounds);
            let pointer = point_to_vec2(position);
            self.hardpoint_labels
                .iter()
                .enumerate()
                .filter_map(|(index, label)| {
                    let anchor = self.viewport.pos_to_world_space(Vec3::from(label.position));
                    let at = self.viewport.camera.project_to_screen(anchor, rect)?;
                    let distance = ((at.x - pointer.x).powi(2) + (at.y - pointer.y).powi(2)).sqrt();
                    (distance <= REACH_PX).then_some((index, distance))
                })
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(index, _)| index)
        });

        let changed = found != self.hovered_hardpoint;
        self.hovered_hardpoint = found;
        changed
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
        let Some(scene) = self.scene() else { return };
        let Some(armor) = self.current_armor.clone() else { return };
        let show_zero_mm = self.show_zero_mm();
        let visibility = VisibilityFilter {
            part: &self.part_visibility,
            plate: &self.plate_visibility,
            show_hidden_only: self.show_hidden_only,
        };
        let mesh_id = picking_ui::upload_plate_highlight(
            &mut self.viewport,
            scene.device(),
            &armor,
            key,
            visibility,
            show_zero_mm,
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
        let Some(scene) = self.scene() else { return };
        let Some(armor) = self.current_armor.clone() else { return };
        let visibility = VisibilityFilter {
            part: &self.part_visibility,
            plate: &self.plate_visibility,
            show_hidden_only: self.show_hidden_only,
        };
        let show_zero_mm = self.show_zero_mm();
        let mesh_id = match &key {
            SidebarHighlightKey::Zone(zone) => picking_ui::upload_zone_highlight(
                &mut self.viewport,
                scene.device(),
                &armor,
                zone,
                visibility,
                show_zero_mm,
            ),
            SidebarHighlightKey::Part(zone, part) => picking_ui::upload_part_highlight(
                &mut self.viewport,
                scene.device(),
                &armor,
                zone,
                part,
                visibility,
                show_zero_mm,
            ),
            SidebarHighlightKey::Plate(pk) => picking_ui::upload_plate_highlight(
                &mut self.viewport,
                scene.device(),
                &armor,
                pk,
                visibility,
                show_zero_mm,
            ),
            SidebarHighlightKey::HullMeshes(names) => hull_picking::upload_hull_highlight(
                &mut self.viewport,
                scene.device(),
                &armor.hull_meshes,
                names,
                picking_ui::SIDEBAR_HIGHLIGHT_COLOR,
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
        self.mesh_seed.armor = false;
        self.reupload_current_armor(cx);
        cx.emit(ViewportChange::Settings);
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

    /// Toggles a zone's `parts` from the popover's zone-header checkbox
    /// (`checked`), or -- when `solo` is set (Ctrl/Cmd-click) -- solos them:
    /// those parts on, every other part off, and every zone's plate overrides
    /// cleared. Ports `tab.rs:4482-4511`. `parts` is every part of the zone,
    /// or while the rail is filtered, the ones it lists.
    pub(crate) fn toggle_zone(
        &mut self,
        zone: String,
        parts: Vec<String>,
        checked: bool,
        solo: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(armor) = self.current_armor.clone() else { return };
        self.snapshot_and_mutate(cx, move |this| {
            if solo {
                for z in &armor.zone_part_plates {
                    for p in &z.parts {
                        let on = z.name == zone && parts.contains(&p.name);
                        this.part_visibility.insert((z.name.clone(), p.name.clone()), on);
                        for &t in &p.plates {
                            this.plate_visibility.remove(&PlateKey {
                                zone: z.name.clone(),
                                material_name: p.name.clone(),
                                thickness_tenths: t,
                            });
                        }
                    }
                }
                return;
            }
            let Some(z) = armor.zone_part_plates.iter().find(|z| z.name == zone) else { return };
            for p in z.parts.iter().filter(|p| parts.contains(&p.name)) {
                this.part_visibility.insert((zone.clone(), p.name.clone()), checked);
                if checked {
                    for &t in &p.plates {
                        this.plate_visibility.remove(&PlateKey {
                            zone: zone.clone(),
                            material_name: p.name.clone(),
                            thickness_tenths: t,
                        });
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
        self.mesh_seed.armor = false;
        self.plate_visibility = prev.plate_visibility;
        self.reupload_current_armor(cx);
        cx.emit(ViewportChange::Settings);
    }

    /// Ctrl/Cmd+Shift+Z or Ctrl/Cmd+R: re-applies the next visibility
    /// snapshot. Ports `armor_viewer::common::handle_undo_redo`'s redo branch.
    pub(crate) fn redo_visibility(&mut self, cx: &mut Context<Self>) {
        let current = self.current_visibility_snapshot();
        let Some(next) = self.undo_stack.redo(current) else { return };
        self.part_visibility = next.part_visibility;
        self.mesh_seed.armor = false;
        self.plate_visibility = next.plate_visibility;
        self.reupload_current_armor(cx);
        cx.emit(ViewportChange::Settings);
    }

    /// Re-uploads `current_armor` honoring the current `part_visibility`/
    /// `plate_visibility`, without moving the camera
    /// (`upload::reupload_armor_plates`). If the raycast-hovered plate is
    /// still visible its highlight is rebuilt against the fresh geometry; if
    /// it just became hidden the hover/tooltip are cleared outright rather
    /// than waiting for the next mouse-move. The popover's sidebar-hover
    /// highlight, if active, is likewise rebuilt against the fresh geometry.
    fn reupload_current_armor(&mut self, cx: &mut Context<Self>) {
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
        // `viewport.clear()` (inside the armor re-upload above) wipes every
        // mesh, hull included -- top the hull back up now that the armor pass
        // is done, keeping the two upload paths decoupled (a hull-only change
        // never lands here, see `reupload_hull`) while staying in sync with
        // whatever `Viewport3D::clear` just destroyed.
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
                // The egui app's own armor carries a waterline offset that is
                // always zero; the orbits sit at the height the trajectory
                // states until a ship is found that says otherwise.
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
        let layers = depth_cues::GridLayers {
            lines: self.display_settings.show_grid,
            marks: self.display_settings.show_length_marks,
        };
        if let Some(written) = self.length_marks.as_ref() {
            depth_cues::upload_waterline_grid(&mut self.viewport, device, written.marks(), layers);
        }

        if self.display_settings.show_ship_center {
            let (vertices, indices) = super::hits::build_center_marker(armor.bounds);
            self.viewport.add_non_pickable_mesh(device, &vertices, &indices, LAYER_OVERLAY);
        }

        if !self.hits.is_empty() {
            let (vertices, indices) = super::hits::build_markers(
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

        // Walked over the same triangles the armor pass just uploaded, so a
        // hidden plate's rim is not reported as a hole in the ship.
        self.gap_count = 0;
        if self.show_gaps {
            let (vertices, indices, count) = gaps::build_gap_mesh(&armor, visibility, display.show_zero_mm);
            self.gap_count = count;
            if !indices.is_empty() {
                self.viewport.add_non_pickable_mesh(device, &vertices, &indices, LAYER_OVERLAY);
            }
        }
        // After the passes that read the visibility filter, which borrows this
        // viewport while these want it back.
        self.apply_perspective(device, &armor);
        self.upload_splash_boxes(device, &armor);
        self.upload_hardpoints();

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
        self.mesh_highlight = None;
        self.revalidate_mesh_hover();
        self.viewport.mark_dirty();
        cx.notify();
    }

    /// Mutates `display_settings` via `mutate` and re-uploads the armor --
    /// every display-settings popover checkbox and slider (`popover.rs`)
    /// goes through this single seam, so no call site can forget the
    /// re-upload these settings need (they affect geometry: edge outlines,
    /// per-vertex alpha, and 0mm filtering).
    pub(crate) fn mutate_display_settings(
        &mut self,
        cx: &mut Context<Self>,
        mutate: impl FnOnce(&mut upload::DisplaySettings),
    ) {
        mutate(&mut self.display_settings);
        self.reupload_current_armor(cx);
        cx.emit(ViewportChange::Settings);
    }

    /// Re-uploads only the hull meshes (`upload_hull::upload_hull_meshes`),
    /// honoring the current `hull_visibility`/[`Self::hull_opaque`].
    /// Never touches armor meshes, visibility overrides, or the camera, and
    /// never calls `Viewport3D::clear()` itself -- a hull-only change stays
    /// cheap even on a ship with a large armor mesh set. A no-op if the GPU
    /// device isn't ready or there is no armor loaded.
    fn reupload_hull(&mut self) {
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
        self.mesh_seed.hull = false;
        self.reupload_hull();
        cx.emit(ViewportChange::Settings);
        cx.notify();
    }

    /// Whether the hull meshes on screen are drawn opaque: the saved setting
    /// for a ship, this pane's own choice for a standalone model.
    pub(crate) fn hull_opaque(&self) -> bool {
        if self.shown().is_some_and(|armor| !armor.is_ship()) {
            self.model_hull_opaque
        } else {
            self.display_settings.hull_opaque
        }
    }

    /// Sets [`Self::hull_opaque`] and re-uploads only the hull (its alpha and
    /// depth-write layer both depend on this flag, see
    /// `upload_hull::upload_hull_meshes`'s doc). Kept separate from
    /// `mutate_display_settings` so toggling hull opacity -- a hull-only
    /// change -- never rebuilds the armor mesh set.
    pub(crate) fn set_hull_opaque(&mut self, opaque: bool, cx: &mut Context<Self>) {
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

    /// Selects (or clears, `id: None`) the active camo scheme and re-uploads
    /// only the hull. Ports the egui app's camo-change flow (`tab.rs:797-847`):
    /// decode `id`'s textures (cache hit skips the decode), look up its
    /// `CamoSchemeInfo` for `uv_transforms`/`use_color_scheme`, composite them
    /// against `armor.hull_textures` via `camo::build_active_camo`, and store
    /// the result as `active_camo_textures`/`active_camo_uvs`, off the UI
    /// thread. A decode
    /// failure, or a selected id with no matching `camo_scheme_infos` entry,
    /// is logged via `tracing::warn!` and treated as stock (empty active-camo
    /// maps) rather than left half-applied. The decode/composite step itself
    /// is [`decode_active_camo`], run on a background thread here and by a
    /// reload ([`reload_camo`]).
    pub(crate) fn select_camo(&mut self, id: Option<CamoSchemeId>, cx: &mut Context<Self>) {
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

        // Decoding a scheme reads and composites several full-size textures,
        // which is far too much work to do between two frames; the hull keeps
        // the camo it has until the new one lands.
        let generation = self.camo_generation;
        let cached = self.camo_texture_cache.get(id);
        cx.spawn(async move |this, cx| {
            let decode = cx.background_spawn(async move { decode_active_camo(id, cached, &armor) }).await;
            let _ = this.update(cx, |this, cx| this.apply_camo_decode(generation, id, decode, cx));
        })
        .detach();
        cx.notify();
    }

    /// Takes a finished camo decode, unless the selection moved on while it
    /// ran.
    fn apply_camo_decode(&mut self, generation: u64, id: CamoSchemeId, decode: CamoDecode, cx: &mut Context<Self>) {
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

    /// Hull-upgrade selector (`popover.rs`): switches to `key`'s hull upgrade,
    /// clears `selected_modules` (its alternatives may differ per hull,
    /// matching the egui app's own selector, `tab.rs:3169`), and reloads. A
    /// no-op if `key` is already selected.
    /// The hull this viewport is showing, for an export seeded from it.
    pub(crate) fn selected_hull(&self) -> Option<String> {
        self.selected_hull.clone()
    }

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
        let Some(source) = self.scene_source.clone() else { return };
        let cancel = self.reload_load.start();
        let task = match source {
            SceneSource::Ship { bundle, param_index, display_name } => {
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
                let task = spawn_reload_ship_armor(bundle, param_index, options, cancel.clone(), cx);
                cx.background_spawn(async move { task.await.map_err(SceneLoadError::from) })
            }
            SceneSource::Model { bundle, models, request } => {
                let request = ModelLoadRequest { lod: self.hull_lod, ..request };
                let task = load_model::spawn_load_model_scene(models, bundle, request, cancel.clone(), cx);
                cx.background_spawn(async move { task.await.map_err(SceneLoadError::from) })
            }
        };

        // The scheme is carried by name across the reload and recomposited
        // with it, off the UI thread, so the reloaded hull is never shown
        // stock first. The reloaded scheme list is read before the reload
        // lands rather than left to the background stage, so a scheme picked
        // meanwhile is found in it too.
        let shown_schemes = self.current_armor.as_ref().map(|armor| armor.camo_schemes().to_vec()).unwrap_or_default();
        let camo = self.selected_camo.map(|id| (id, shown_schemes));
        let task = cx.background_spawn(async move {
            let FirstStage { scene, mut rest } = task.await?;
            if let Some(at) = rest.iter().position(|job| job.piece() == RestPiece::Camo) {
                match rest.remove(at).run(&scene, &cancel) {
                    Ok(()) => {}
                    Err(RestJobError::Cancelled(cancelled)) => return Err(SceneLoadError::from(cancelled)),
                    Err(e) => tracing::warn!("armor viewer: {} lacks its camouflage: {e}", scene.display_name),
                }
            }
            let camo = camo.map(|(was, infos)| reload_camo(was, &infos, &scene));

            Ok(ReloadedScene { armor: scene, camo, rest })
        });

        self.reload_generation += 1;
        let generation = self.reload_generation;
        let camo_generation = self.camo_generation;
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| this.apply_reload_result(generation, camo_generation, result, cx));
        })
        .detach();
    }

    /// Reads a model's armor thickness from another ship mounting it, keeping
    /// the view as it is.
    pub(crate) fn set_thickness_source(&mut self, mount: MountRef, cx: &mut Context<Self>) {
        let Some(SceneSource::Model { request, .. }) = &mut self.scene_source else { return };
        if request.thickness_from.as_ref() == Some(&mount) {
            return;
        }
        request.thickness_from = Some(mount);
        self.reload_ship(cx);
    }

    /// Opens the export-confirm panel (rail and context-menu "Export",
    /// `popover.rs`'s `render_export_button`) for this pane's currently
    /// displayed ship, snapshotting `reload_source` plus the live hull/LOD/
    /// module selection into `export_confirm` so the export target is FIXED
    /// for the panel's lifetime -- see `ExportConfirm`'s doc. A no-op if no
    /// ship is loaded -- the button itself is disabled in that case
    /// (`current_armor.is_none()`), this is just defense in depth against a
    /// stale click.
    pub(crate) fn open_export_confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.current_armor.is_none() {
            return;
        }
        let source = match &self.scene_source {
            Some(SceneSource::Ship { bundle, param_index, display_name }) => (bundle, param_index, display_name),
            Some(SceneSource::Model { request, .. }) => {
                // A model has no hull or camo to confirm; the pane's export
                // dialog asks what it should contain.
                let request = request.clone();
                if let Some(pane) = self.pane() {
                    pane.update(cx, |pane, cx| pane.export_catalog_model(request, window, cx));
                }
                return;
            }
            None => return,
        };
        let (bundle, param_index, display_name) = source;
        self.export_confirm = Some(ExportConfirm {
            bundle: Arc::clone(bundle),
            param_index: param_index.clone(),
            display_name: display_name.clone(),
            hull_lod: self.hull_lod,
            selected_hull: self.selected_hull.clone(),
            selected_modules: self.selected_modules.clone(),
        });
        cx.notify();
    }

    /// Dismisses the export-confirm panel with no further action, matching
    /// egui's own Cancel button (`tab.rs:1102-1104`).
    pub(crate) fn cancel_export(&mut self, cx: &mut Context<Self>) {
        self.export_confirm = None;
        cx.notify();
    }

    /// Confirms the export: closes the confirm panel, then asks where to write
    /// `{display_name}.glb` (`crate::dialog`, which keeps the app drawing
    /// while the dialog is open). A cancelled dialog is a no-op beyond
    /// closing the panel. On a chosen path, builds `ShipExportOptions` from the
    /// SNAPSHOT taken when the panel opened (`export_confirm`, `load_ship::
    /// export_options_from_selection`) -- never from this pane's possibly-
    /// since-changed live hull/LOD/module selection, so the sidebar staying
    /// interactive while this dialog is open can't retarget the export onto
    /// a different ship -- and runs the actual export (`load_ship_from_vehicle`
    /// and `export_glb`, both CPU/IO heavy) on the background executor so the
    /// UI thread is not blocked, matching every other `ShipAssets`-driven
    /// load in this pane (`reload_ship` above). Ports the egui app's Export
    /// button handler (`tab.rs:1057-1097`), except the LOD/hull/module
    /// options reflect the pane's selection AS OF OPEN TIME rather than
    /// egui's fixed `lod: 0` -- see `load_ship::export_options_from_selection`'s
    /// doc. Textures are embedded either way (`textures: true`).
    ///
    /// Reports the outcome in the words `ArmorViewerPane::export_model` uses
    /// for the same export, since a written file is otherwise invisible.
    pub(crate) fn confirm_export(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(snapshot) = self.export_confirm.take() else { return };
        cx.notify();
        let asked = crate::dialog::save_file(
            None,
            &load_ship::default_export_filename(&snapshot.display_name),
            Some(crate::dialog::GLB),
        );

        let bundle = snapshot.bundle;
        let param_index = snapshot.param_index;
        let display_name = snapshot.display_name;
        let options = load_ship::export_options_from_selection(
            snapshot.hull_lod,
            snapshot.selected_hull,
            snapshot.selected_modules,
        );

        cx.spawn_in(window, async move |this, cx| {
            let Some(path) = crate::dialog::chosen(asked, &this, cx).await else { return };
            let ship = display_name.clone();
            let written = cx
                .background_spawn(async move {
                    let result = load_ship::export_ship_glb(&bundle.assets, &param_index, &options, &path);
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

    /// Applies a completed `reload_ship` result. Discards it if a newer
    /// reload superseded this one (`generation` guard, mirroring
    /// `ArmorViewerPane::apply_ship_load_result`'s identical pattern for ship
    /// selection). Otherwise: adopts the selected camo as the reload found
    /// it by name ([`reload_camo`]) and clears `camo_texture_cache`, since
    /// `CamoSchemeId` is only a stable index within a single load's
    /// `camo_scheme_infos` list -- see that type's own doc -- and this reload
    /// rebuilds the list from scratch; retains whatever `hull_visibility`/
    /// `part_visibility`/`plate_visibility` entries still apply to the new
    /// armor (`visibility::retain_*`; a hull/LOD/module change can add or
    /// drop parts/plates/hull meshes); adopts the camo composited against
    /// the new armor's hull textures in the background;
    /// replaces `current_armor`; and re-uploads armor + hull WITHOUT
    /// reframing the camera (`upload::reupload_armor_plates`, not
    /// `upload_armor_to_viewport`) -- the user is mid-view when they change a
    /// hull/LOD/module selector, and a reload must not jump it. Clears the
    /// undo/redo history and any hover/sidebar highlight, since either can
    /// reference a part/plate/mesh the reload just removed, matching the egui
    /// app's own `apply_upgrade_reload` (`tab.rs:2737`).
    fn apply_reload_result(
        &mut self,
        generation: u64,
        camo_generation: u64,
        result: Result<ReloadedScene, SceneLoadError>,
        cx: &mut Context<Self>,
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

        // A scheme picked while the reload ran replaces the one it carried,
        // and is found by name in the reloaded scene once that is shown.
        let picked_since = (camo_generation != self.camo_generation).then(|| {
            let old_infos: &[CamoSchemeInfo] = self.current_armor.as_ref().map(|old| old.camo_schemes()).unwrap_or(&[]);
            self.selected_camo.and_then(|id| remap_camo_selection(old_infos, armor.camo_schemes(), id))
        });
        // Keyed by the pre-reload ids, which are no longer valid indices
        // into the new `camo_scheme_infos` list -- a stale-keyed hit here
        // would skip decode and apply a different scheme's textures.
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
        cx.emit(ViewportChange::Settings);
        cx.notify();
    }

    /// Applies a lighting preset (In-game/Flat/Studio popover buttons):
    /// replaces `viewport.lighting` wholesale but keeps `enabled` across the
    /// swap, matching the egui original's own preset handlers (`tab.rs:5055-
    /// 5075`, each of which reads `pane.lighting.enabled` before overwriting
    /// `pane.lighting` and restores it after). A preset moves every lighting
    /// slider's thumb at once, so the sliders are rebuilt (not `set_value`d
    /// -- see `LightingSliders`'s doc) rather than just the domain value.
    pub(crate) fn set_lighting_preset(
        &mut self,
        mut preset: LightingSettings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        preset.enabled = self.viewport.lighting.enabled;
        self.viewport.lighting = preset.clone();
        let (lighting_sliders, subs) = Self::build_lighting_sliders(cx, &preset);
        self.lighting_sliders = lighting_sliders;
        self._lighting_slider_subscriptions = subs;
        // The pickers follow the preset rather than being rebuilt: a picker the
        // reader has open stays open, showing the colour the preset just set.
        let (flat, key) = (rgb_color(preset.flat_color), rgb_color(preset.key_color));
        self.lighting_colors.flat.update(cx, |picker, cx| picker.set_value(flat, window, cx));
        self.lighting_colors.key.update(cx, |picker, cx| picker.set_value(key, window, cx));
        self.viewport.mark_dirty();
        cx.emit(ViewportChange::Settings);
        cx.notify();
    }

    fn snap_camera(&mut self, axis: Axis, positive: bool, cx: &mut Context<Self>) {
        let (az, el) = camera::ortho_view(axis, positive, self.viewport.camera.azimuth);
        self.viewport.camera.animate_to(az, el, GIZMO_SNAP_DURATION_SECS);
        self.viewport.mark_dirty();
        cx.emit(ViewportChange::Camera);
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
                    // Every frame of the snap is a camera change, so a
                    // mirrored pane follows the whole eased move rather than
                    // sitting at the old orientation until the next drag.
                    cx.emit(ViewportChange::Camera);
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
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_gpu(cx);

        let status: Option<String> = match &self.gpu {
            GpuState::Waiting => Some(t!("ui.armor.initializing").into_owned().to_string()),
            GpuState::Failed(reason) => Some(t!("ui.armor.init_failed", reason = reason).to_string()),
            GpuState::Ready { .. } => None,
        };
        // Nothing is drawn until a ship is picked, so the viewport carries
        // the egui app's own two messages instead (`ui.armor.loading_armor`
        // and `ui.armor.select_ship`, hardcoded like the rest of this
        // crate's strings -- see `replay_inspector::browser_view`). A load in
        // flight replaces whatever is drawn, as it does in the egui pane, so
        // the ship on screen is never mistaken for the one being loaded.
        let loading_ship = self.ship_loading.clone();
        let problem = self.scene_problem.clone().filter(|_| loading_ship.is_none() && status.is_none());
        let empty_state = (self.current_armor.is_none() && loading_ship.is_none() && problem.is_none())
            .then(|| status.clone().unwrap_or_else(|| t!("ui.armor.select_ship").into_owned()));
        let scene_shown = loading_ship.is_none() && problem.is_none() && empty_state.is_none() && status.is_none();
        let image_child = if let Some(name) = loading_ship {
            crate::loading::Loading::new(
                "armor-viewport-loading",
                t!("ui.armor.loading_named_ship", ship = name).to_string(),
            )
            .into_any_element()
        } else if matches!(self.gpu, GpuState::Waiting) {
            // Also covers a lost device not yet replaced, which has no end.
            crate::loading::Loading::new("armor-viewport-initializing", t!("ui.armor.initializing").into_owned())
                .unbounded()
                .into_any_element()
        } else if let Some(problem) = problem {
            v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .gap_1()
                .child(div().text_sm().child(problem.headline()))
                .child(div().text_sm().text_color(crate::theme::text_dim()).child(problem.reason.explanation()))
                .into_any_element()
        } else if let Some(message) = empty_state.or(status) {
            h_flex()
                .size_full()
                .items_center()
                .justify_center()
                .child(div().text_sm().text_color(crate::theme::text_dim()).child(message))
                .into_any_element()
        } else {
            // The scene itself is painted by `overlay`, under the gizmo.
            div().size_full().into_any_element()
        };

        let camera_snapshot = self.viewport.camera.clone();
        // The gizmo orients a model; with none loaded there is nothing for it
        // to orient, and the egui pane draws it inside the has-a-ship branch
        // only (`ui/tab.rs`'s `draw_gizmo` call).
        let gizmo_drawn = self.current_armor.is_some() && self.scene_problem.is_none();
        let hover = self.gizmo_hover;
        let weak = cx.weak_entity();
        let overlay = canvas(
            {
                let weak = weak.clone();
                move |bounds: Bounds<Pixels>, _window, cx| {
                    let _ = weak.update(cx, |this, _cx| this.last_bounds = Some(bounds));
                }
            },
            move |bounds, (), window, cx| {
                if scene_shown {
                    let _ = weak.update(cx, |this, cx| {
                        this.paint_scene(bounds, window);
                        depth_cues::paint_vignette(bounds, window);
                        this.rewrite_length_marks();
                        this.paint_annotations(bounds, window, cx);
                        this.paint_hit_pulses(bounds, window, cx);
                    });
                }
                if gizmo_drawn {
                    draw_gizmo_overlay(bounds, &camera_snapshot, hover, window, cx);
                }
            },
        )
        // `inset_0`, not `size_full`: an absolute element with no insets is
        // laid out at its static position, which is below the image it is
        // meant to cover. That put the overlay (and with it the bounds the
        // gizmo is drawn in and every pick is cast against) a whole viewport
        // below the viewport.
        .absolute()
        .inset_0();

        // Floating thickness tooltip, anchored near (not under) the cursor
        // and snapped back inside the window if it would overflow -- reuses
        // gpui's own `anchored()`/`deferred()` primitives (the same ones the
        // context menu below and gpui-component's managed tooltips use)
        // rather than hand-measuring the tooltip's size.
        let ring_overlay = self.hovered_ring.as_ref().map(|(label, cursor)| {
            let anchor_pos = point(cursor.x + TOOLTIP_CURSOR_OFFSET, cursor.y + TOOLTIP_CURSOR_OFFSET);
            let element = crate::ui::overlay_surface(div(), cx)
                .px_2()
                .py_1()
                .text_xs()
                .children(label.lines().map(|line| div().child(line.to_string())));
            deferred(anchored().position(anchor_pos).snap_to_window_with_margin(px(8.)).child(element))
                .with_priority(0)
                .into_any_element()
        });

        let tooltip_overlay = self.hovered.as_ref().filter(|_| self.hovered_ring.is_none()).map(|hover| {
            let theme = cx.theme();
            let (background, border, radius, muted) =
                (theme.background, theme.border, theme.radius, theme.muted_foreground);
            // A zone and a plate are named the way the game names them, as the
            // egui tooltip does.
            let zone_label = match self.reload_bundle() {
                Some(bundle) => super::catalog::translate_name(bundle.assets.metadata(), &hover.tooltip.zone),
                None => hover.tooltip.zone.clone(),
            };
            let material_label = self.material_label(hover.tooltip.material);
            let hint = t!("ui.armor.cast_here_hint", key = crate::status_bar::ModifierKey::Shift.label(cx));
            let element = picking_ui::tooltip_element(
                &hover.tooltip,
                background,
                border,
                radius,
                muted,
                picking_ui::TooltipLabels { zone: &zone_label, material: &material_label, hint: &hint },
            );
            let anchor_pos = point(hover.cursor.x + TOOLTIP_CURSOR_OFFSET, hover.cursor.y + TOOLTIP_CURSOR_OFFSET);
            deferred(anchored().position(anchor_pos).snap_to_window_with_margin(px(8.)).child(element))
                .with_priority(0)
                .into_any_element()
        });

        let mesh_tooltip_overlay = self.hovered_mesh.as_ref().filter(|_| self.hovered_ring.is_none()).map(|hover| {
            let anchor_pos = point(hover.cursor.x + TOOLTIP_CURSOR_OFFSET, hover.cursor.y + TOOLTIP_CURSOR_OFFSET);
            deferred(
                anchored().position(anchor_pos).snap_to_window_with_margin(px(8.)).child(hover.tooltip.element(cx)),
            )
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
        let hovered_for_menu = self.hovered.clone().map(|hover| {
            let label = self.material_label(hover.tooltip.material);
            (hover, label)
        });
        let hover_is_hidden =
            hovered_for_menu.as_ref().is_some_and(|(h, _)| self.plate_visibility.get(&h.key).copied().unwrap_or(false));
        let hidden_count = self.plate_visibility.len();

        // Export-confirm panel (Milestone 5 Task 10): a minimal inline
        // overlay (no `Modal`/`ContextModal` exists in this pinned
        // gpui-component revision) reproducing egui's own `export_confirm`
        // dialog (`tab.rs:1045-1111`) -- ship name, disclaimer, Export/Cancel.
        // `.occlude()` blocks the 3D viewport's own mouse handlers underneath
        // while it's open, matching `legend.rs`'s dragged panel.
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
                        .shadow(crate::theme::overlay_shadow())
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
            .children((!self.hits.is_empty()).then(|| {
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
                    .children(self.shot_log().into_iter().map(|line| {
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
            .children(self.length_readout().filter(|_| scene_shown).map(|readout| {
                div()
                    .absolute()
                    .bottom_2()
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
            .when_some(mesh_tooltip_overlay, |this, t| this.child(t))
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
                let disable_material_name = material;
                menu =
                    menu.item(PopupMenuItem::new(format!("Disable {label}")).on_click(move |_event, _window, cx| {
                        disable_entity
                            .update(cx, |view, cx| view.disable_material(&disable_zone, &disable_material_name, cx));
                    }));

                popover::quick_toggle_items(menu.separator(), &entity, cx)
            })
    }
}

/// Resolves a selected camo scheme across a full reload (Task 8c) by name.
/// `CamoSchemeId` is a raw index into a per-load `camo_scheme_infos` list
/// (see that type's own doc: "never persisted") -- a reload rebuilds the list
/// from scratch, so `old_id` can end up pointing at an entirely different
/// scheme in `new_infos`, or at nothing at all. Looks up `old_id`'s
/// `display_name` in `old_infos`, then finds the entry with that same
/// `display_name` in `new_infos` and returns its (new) id. Returns `None` if
/// `old_id` has no entry in `old_infos`, or if no entry in `new_infos` shares
/// its name -- the camo did not survive the reload (for example, a hull
/// upgrade dropped a ship-specific scheme).
fn remap_camo_selection(
    old_infos: &[CamoSchemeInfo],
    new_infos: &[CamoSchemeInfo],
    old_id: CamoSchemeId,
) -> Option<CamoSchemeId> {
    let name = &old_infos.iter().find(|i| i.id == old_id)?.display_name;
    new_infos.iter().find(|i| &i.display_name == name).map(|i| i.id)
}

/// A reloaded scene, with the camo that was selected composited onto it and
/// the background jobs that finish it.
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

/// Finds `was` in `armor` by its name in `old_infos`, and composites it.
/// Decodes and composites full-size textures, so it runs off the UI thread.
fn reload_camo(was: CamoSchemeId, old_infos: &[CamoSchemeInfo], armor: &LoadedShipArmor) -> ReloadedCamo {
    let now = remap_camo_selection(old_infos, armor.camo_schemes(), was);
    let decode = match now {
        Some(id) => decode_active_camo(id, None, armor),
        None => CamoDecode { decoded: None, textures: HashMap::new(), uvs: HashMap::new() },
    };

    ReloadedCamo { was, now, decode }
}

/// How many schemes' decoded textures are kept. One scheme is several
/// full-size textures, so a reader stepping through a ship's camo list would
/// otherwise hold every one of them until the ship changes.
const CAMO_TEXTURE_CACHE_SIZE: usize = 3;

/// Decoded scheme textures, most recently used last.
///
/// Shared by `Arc`: a hit hands the decoder the same textures rather than a
/// copy of every one of them, which is what reselecting a scheme used to cost.
#[derive(Default)]
struct CamoTextureCache {
    entries: Vec<(CamoSchemeId, Arc<SchemeTextures>)>,
}

impl CamoTextureCache {
    fn get(&mut self, id: CamoSchemeId) -> Option<Arc<SchemeTextures>> {
        let at = self.entries.iter().position(|(held, _)| *held == id)?;
        let entry = self.entries.remove(at);
        let textures = Arc::clone(&entry.1);
        self.entries.push(entry);
        Some(textures)
    }

    fn insert(&mut self, id: CamoSchemeId, textures: Arc<SchemeTextures>) {
        self.entries.retain(|(held, _)| *held != id);
        self.entries.push((id, textures));
        while self.entries.len() > CAMO_TEXTURE_CACHE_SIZE {
            self.entries.remove(0);
        }
    }

    fn clear(&mut self) {
        self.entries.clear();
    }
}

/// Per-hull-part composited camo textures, keyed as
/// [`upload_hull::upload_hull_meshes`] expects.
type CamoTextures = super::camo::TexturesByPart;

/// One scheme's decoded and composited textures.
struct CamoDecode {
    /// The scheme's own textures, when this run decoded them; a run that was
    /// handed them from the cache leaves this `None`, since the caller
    /// already holds them.
    decoded: Option<Arc<SchemeTextures>>,
    /// Empty means "render base albedo only", which is what stock looks
    /// like.
    textures: CamoTextures,
    uvs: HashMap<String, UvTransform>,
}

/// Decodes `id`'s textures (unless `cached` already holds them) and
/// composites them against `armor`'s hull textures. A decode failure, or an
/// id with no matching `camo_scheme_infos` entry, is logged via
/// `tracing::warn!` and treated as stock rather than left half-applied.
///
/// Takes no `&mut` state, so it runs on a background thread
/// ([`ViewportView::select_camo`], [`reload_camo`]).
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

/// What releasing `button` does. A press that moved past the drag threshold
/// was an orbit or a pan, and does nothing more.
///
/// Shift-click casts: a plain left click hides a plate, a left drag orbits,
/// a middle drag pans, a right click opens the menu, and Ctrl-click is a
/// right click on macOS, so Shift is the modifier no other gesture holds. It
/// is read before splash mode, which owns the plain click only.
fn click_action(
    button: MouseButton,
    plain_click: bool,
    press_in_gizmo: bool,
    splash_mode: bool,
    shift: bool,
) -> ClickAction {
    if !plain_click {
        return ClickAction::Nothing;
    }

    match button {
        MouseButton::Left if press_in_gizmo => ClickAction::SnapGizmo,
        MouseButton::Left if shift => ClickAction::Cast,
        MouseButton::Left if splash_mode => ClickAction::Burst,
        MouseButton::Left => ClickAction::TogglePlate,
        _ => ClickAction::Nothing,
    }
}

/// The direction a shell travels, given the bearing it comes in on.
///
/// With a shell to fire the fall angle is the solver's; without one the
/// shell comes in flat, which is what the lead-in line then shows.
fn fall_direction(approach: Vec3, impact: Option<&ImpactResult>) -> Vec3 {
    match impact {
        Some(impact) => {
            let fall = impact.impact_angle_horizontal.value().to_radians();
            Vec3::new(approach[0] * fall.cos(), -fall.sin(), approach[2] * fall.cos()).normalize()
        }
        None => approach,
    }
}

/// Paints a label centred above where it is anchored, on a dark plate so it
/// reads against any hull, in the egui viewer's colours, at `opacity`.
fn paint_box_label(name: &str, anchor: Point<Pixels>, opacity: f32, window: &mut Window, cx: &mut App) {
    let run = TextRun {
        len: name.len(),
        font: window.text_style().font(),
        color: Hsla::from(rgba(0xc8dcffdc)).opacity(opacity),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let line_height = px(13.0);
    let shaped = window.text_system().shape_line(SharedString::from(name.to_string()), px(11.0), &[run], None);
    let origin = point(anchor.x - shaped.width() * 0.5, anchor.y - line_height - px(2.0));

    let plate = Bounds { origin, size: size(shaped.width(), line_height) }.dilate(px(2.0));
    window.paint_quad(fill(plate, Hsla::from(rgba(0x0000008c)).opacity(opacity)).corner_radii(px(2.0)));
    let _ = shaped.paint(origin, line_height, TextAlign::Left, None, window, cx);
}

/// Paints one cast shell: each line over a dark outline so it reads against
/// any hull, a ringed dot at each plate reached in its strike-angle colour,
/// and a larger one in the arc's own colour where the fuse went off.
fn paint_arc(
    drawing: &trajectory::ArcDrawing,
    color: [f32; 4],
    selected: bool,
    project: impl Fn(Vec3) -> Option<Point<Pixels>>,
    window: &mut Window,
) {
    let width = if selected { SELECTED_ARC_LINE_WIDTH } else { ARC_LINE_WIDTH };
    let line_color = swatch_color(color);

    for line in &drawing.lines {
        let opacity = match line.stroke {
            trajectory::Stroke::Solid => 1.0,
            trajectory::Stroke::Faded => 0.45,
        };
        for run in visible_runs(&line.points, &project) {
            paint_polyline(&run, width + ARC_OUTLINE * 2.0, black().opacity(0.8 * opacity), window);
            paint_polyline(&run, width, line_color.opacity(opacity), window);
        }
    }

    for impact in &drawing.impacts {
        if let Some(at) = project(impact.position) {
            paint_dot(at, IMPACT_RADIUS, swatch_color(impact.color), window);
        }
    }

    if let Some(at) = drawing.detonation.and_then(&project) {
        paint_dot(at, BURST_RADIUS, line_color, window);
    }
}

/// The stretches of `points` in front of the camera that are long enough to
/// draw. A point behind the camera has no place on screen, so the line is
/// broken there rather than joined across it.
fn visible_runs(points: &[Vec3], project: &impl Fn(Vec3) -> Option<Point<Pixels>>) -> Vec<Vec<Point<Pixels>>> {
    let mut runs = Vec::new();
    let mut run = Vec::new();
    for point in points {
        match project(*point) {
            Some(at) => run.push(at),
            None if run.len() >= 2 => runs.push(std::mem::take(&mut run)),
            None => run.clear(),
        }
    }

    if run.len() >= 2 {
        runs.push(run);
    }
    runs
}

fn paint_polyline(points: &[Point<Pixels>], width: Pixels, color: Hsla, window: &mut Window) {
    let Some((first, rest)) = points.split_first() else { return };

    let mut path = PathBuilder::stroke(width);
    path.move_to(*first);
    for point in rest {
        path.line_to(*point);
    }
    if let Ok(path) = path.build() {
        window.paint_path(path, color);
    }
}

/// A filled dot ringed in black, which reads on any colour under it.
fn paint_dot(center: Point<Pixels>, radius: Pixels, color: Hsla, window: &mut Window) {
    let bounds = Bounds { origin: point(center.x - radius, center.y - radius), size: size(radius * 2.0, radius * 2.0) };
    window.paint_quad(quad(bounds, radius, color, px(1.5), black(), BorderStyle::default()));
}

#[cfg(test)]
mod all_visible_tests {
    use super::AllVisible;
    use super::PlateKey;
    use std::collections::HashMap;

    /// The derivation, read off the three maps without a viewport to hold
    /// them: this is the rule that decides what the next ship opens showing.
    fn all_visible(
        hull: &HashMap<String, bool>,
        parts: &HashMap<(String, String), bool>,
        plates: &HashMap<PlateKey, bool>,
    ) -> AllVisible {
        AllVisible {
            hull: !hull.is_empty() && hull.values().all(|visible| *visible),
            armor: parts.values().all(|visible| *visible) && !plates.values().any(|hidden| *hidden),
        }
    }

    fn hull(entries: &[(&str, bool)]) -> HashMap<String, bool> {
        entries.iter().map(|(name, visible)| ((*name).to_string(), *visible)).collect()
    }

    #[test]
    fn nothing_hidden_reads_as_everything_shown() {
        let shown = all_visible(&hull(&[("Hull_A", true)]), &HashMap::new(), &HashMap::new());
        assert_eq!(shown, AllVisible { hull: true, armor: true });
    }

    #[test]
    fn a_hull_mesh_with_no_entry_is_not_shown() {
        // Absent means hidden on the hull side, so an empty map is not
        // "everything".
        let shown = all_visible(&HashMap::new(), &HashMap::new(), &HashMap::new());
        assert!(!shown.hull);
        assert!(shown.armor, "a part with no entry is visible, so an empty map is everything");
    }

    #[test]
    fn one_hidden_part_or_plate_is_enough_to_say_not_everything() {
        let parts = HashMap::from([(("Zone".to_string(), "Part".to_string()), false)]);
        assert!(!all_visible(&HashMap::new(), &parts, &HashMap::new()).armor);

        let plates: HashMap<PlateKey, bool> = HashMap::from([(
            PlateKey { zone: "Zone".to_string(), material_name: "Part".to_string(), thickness_tenths: 0 },
            true,
        )]);
        assert!(!all_visible(&HashMap::new(), &HashMap::new(), &plates).armor);
    }
}

#[cfg(test)]
mod tests {
    use super::PlateKey;
    use std::collections::HashMap;

    use super::Axis;
    use super::ClickAction;
    use super::HeldMotion;
    use super::MoveKey;
    use super::SyncedSettings;
    use super::axis_color;
    use super::ball_bounds;
    use super::click_action;
    use super::point_to_vec2;
    use super::remap_camo_selection;
    use super::vec2_to_point;
    use super::view_rect_from_bounds;
    use crate::armor_viewer::upload::DisplaySettings;
    use crate::viewport::types::LightingSettings;
    use gpui_kit::Bounds;
    use gpui_kit::MouseButton;
    use gpui_kit::point;
    use gpui_kit::px;
    use gpui_kit::size;
    use wowsunpack::export::camo_textures::CamoSchemeId;
    use wowsunpack::export::camo_textures::CamoSchemeInfo;
    use wowsunpack::export::gltf_export::CamoOrigin;

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
    fn shift_arrows_pan_instead_of_orbit_and_rise() {
        let held: Vec<MoveKey> = ["left", "up"].iter().filter_map(|key| MoveKey::from_key(key, true)).collect();
        let motion = HeldMotion::from_keys(&held);
        assert_eq!(motion, HeldMotion { pan_x: -1.0, pan_y: 1.0, ..HeldMotion::default() });

        let held: Vec<MoveKey> = ["left", "up"].iter().filter_map(|key| MoveKey::from_key(key, false)).collect();
        let motion = HeldMotion::from_keys(&held);
        assert_eq!(motion, HeldMotion { rotate: 1.0, vertical: 1.0, ..HeldMotion::default() });
    }

    /// Shift released before the arrow must not leave the pan running.
    #[test]
    fn releasing_an_arrow_stops_its_pan_whatever_shift_is() {
        let pan = MoveKey::from_key("right", true).expect("shift+right is a movement key");
        assert!(MoveKey::released_by("right").any(|key| key == pan));
    }

    /// Shift-click casts a shell with nothing to switch on first, in splash
    /// mode too; a plain click keeps hiding plates.
    #[test]
    fn a_shift_click_casts_and_a_plain_click_hides_a_plate() {
        assert_eq!(click_action(MouseButton::Left, true, false, false, true), ClickAction::Cast);
        assert_eq!(click_action(MouseButton::Left, true, false, true, true), ClickAction::Cast);
        assert_eq!(click_action(MouseButton::Left, true, false, false, false), ClickAction::TogglePlate);
        assert_eq!(click_action(MouseButton::Left, true, false, true, false), ClickAction::Burst);
    }

    /// A Shift-drag is still an orbit, and the gizmo still snaps under Shift.
    #[test]
    fn a_shift_drag_orbits_and_the_gizmo_snaps_without_casting() {
        assert_eq!(click_action(MouseButton::Left, false, false, false, true), ClickAction::Nothing);
        assert_eq!(click_action(MouseButton::Left, true, true, false, true), ClickAction::SnapGizmo);
    }

    /// The middle button pans, so neither a middle click nor a middle drag
    /// casts.
    #[test]
    fn the_middle_button_never_casts() {
        assert_eq!(click_action(MouseButton::Middle, true, false, false, false), ClickAction::Nothing);
        assert_eq!(click_action(MouseButton::Middle, true, false, false, true), ClickAction::Nothing);
        assert_eq!(click_action(MouseButton::Middle, false, false, false, true), ClickAction::Nothing);
    }

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

    #[test]
    fn remap_camo_selection_follows_name_across_shuffled_ids() {
        let old = vec![scheme(0, "Camo A", CamoOrigin::ShipSpecific), scheme(1, "Camo B", CamoOrigin::Universal)];
        // The reload rebuilt the scheme list in a different order: "Camo A"
        // is now id 1, not id 0 -- exactly the instability this guards
        // against.
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

    /// The reload finds the selected scheme by name in the reloaded scene,
    /// on the thread that loaded it.
    #[test]
    fn a_reload_carries_the_selected_camo_by_name() {
        let old = vec![scheme(0, "Camo A", CamoOrigin::ShipSpecific), scheme(1, "Camo B", CamoOrigin::Universal)];
        let path =
            wowsunpack::export::model_catalog::ModelPath::parse("content/gameplay/usa/gun/main/AGM034/AGM034.model")
                .expect("a model path");
        let mut armor = crate::armor_viewer::load_model::fixtures::model_scene(path, Vec::new());
        armor.camo_scheme_infos =
            vec![scheme(0, "Camo B", CamoOrigin::Universal), scheme(1, "Camo A", CamoOrigin::ShipSpecific)].into();

        let reloaded = super::reload_camo(CamoSchemeId(0), &old, &armor);

        assert_eq!((reloaded.was, reloaded.now), (CamoSchemeId(0), Some(CamoSchemeId(1))));
        assert!(super::reload_camo(CamoSchemeId(5), &old, &armor).now.is_none(), "an unknown scheme reverts to stock");
    }

    fn sample_synced_settings() -> SyncedSettings {
        let mut part_visibility = HashMap::new();
        part_visibility.insert(("Citadel".to_string(), "Belt".to_string()), false);
        let mut plate_visibility = HashMap::new();
        plate_visibility.insert(
            PlateKey { zone: "Citadel".to_string(), material_name: "Belt".to_string(), thickness_tenths: 200 },
            true,
        );
        let mut hull_visibility = HashMap::new();
        hull_visibility.insert("Deck".to_string(), true);
        SyncedSettings {
            part_visibility,
            plate_visibility,
            hull_visibility,
            display_settings: DisplaySettings { show_zero_mm: true, hull_opaque: true, ..Default::default() },
            lighting: LightingSettings::flat(),
            camera_rings: crate::armor_viewer::viewport_view::CameraRingSettings { shown: true, ..Default::default() },
            perspective: crate::armor_viewer::viewport_view::PerspectiveSettings {
                enabled: true,
                ..Default::default()
            },
            mesh_seed: Default::default(),
        }
    }

    /// `ViewportView::synced_settings`/`apply_synced` are a straight field
    /// copy into/out of this struct (see their docs) -- this is the pure,
    /// `Context`-free half of that guarantee: a captured snapshot survives a
    /// clone (the same operation `pane.rs`'s mirror/sync broadcast performs
    /// when handing a source pane's settings to every other pane) with every
    /// field intact.
    #[test]
    fn synced_settings_round_trips_through_clone() {
        let original = sample_synced_settings();
        let applied = original.clone();
        assert_eq!(original, applied);
    }

    #[test]
    fn synced_settings_differ_when_a_field_differs() {
        let mut other = sample_synced_settings();
        other.hull_visibility.insert("Deck".to_string(), false);
        assert_ne!(sample_synced_settings(), other);
    }
}

#[cfg(test)]
mod opening_visibility_tests {
    use super::AllVisible;
    use super::OpeningLayer;
    use super::OpeningVisibility;
    use std::collections::HashMap;

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

    fn turret() -> wowsunpack::export::model_catalog::ModelPath {
        wowsunpack::export::model_catalog::ModelPath::parse("content/gameplay/usa/gun/main/AGM034/AGM034.model")
            .expect("a model path")
    }

    #[test]
    fn an_armored_model_opens_on_the_chosen_layer() {
        let mut scene = crate::armor_viewer::load_model::fixtures::armored_model_scene(turret());
        if let crate::armor_viewer::load_ship::SceneSubject::Model(model) = &mut scene.subject {
            model.thickness =
                crate::armor_viewer::load_ship::ArmorThickness::Ship(crate::armor_viewer::load_ship::ThicknessSource {
                    vehicle_param_index: crate::armor_viewer::model_refs::VehicleParamIndex::new("PASC001".to_string()),
                    display_name: "Iowa".to_string(),
                    hp: wowsunpack::export::hardpoint::HardpointName::new("HP_AGM_1"),
                });
        }

        assert_eq!(OpeningLayer::Armor.for_scene(&scene), OpeningLayer::Armor);
        assert_eq!(OpeningLayer::Mesh.for_scene(&scene), OpeningLayer::Mesh);
    }

    #[test]
    fn a_model_with_armor_of_unknown_thickness_opens_on_its_meshes() {
        let scene = crate::armor_viewer::load_model::fixtures::armored_model_scene(turret());

        assert_eq!(OpeningLayer::Armor.for_scene(&scene), OpeningLayer::Mesh);
    }

    #[test]
    fn a_model_without_armor_opens_on_its_meshes_and_shows_them() {
        let scene = crate::armor_viewer::load_model::fixtures::model_scene(turret(), Vec::new());
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

    /// A model's camera stands far enough off that the whole of what is
    /// drawn fits the view, and a hidden layer reaching further is ignored.
    #[test]
    fn a_model_is_framed_on_the_layers_drawn() {
        let mut scene = crate::armor_viewer::load_model::fixtures::armored_model_scene(turret());
        for position in &mut scene.meshes[0].positions {
            *position = position.map(|axis| axis * 100.0);
        }
        let shown = super::ShownLayers { armor: false, hull: true };

        let camera = super::opening_camera(&scene, shown, 0.5);

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

    use wowsunpack::export::mount_armor::MountArmorIndex;
    use wowsunpack::export::ship::ShipAssets;
    use wowsunpack::game_params::provider::GameMetadataProvider;

    use super::ShownLayers;
    use super::opening_camera;
    use crate::armor_viewer::assets::ModelAssets;
    use crate::armor_viewer::hardpoint_overlay::HardpointMarkers;
    use crate::armor_viewer::load_model::ModelLoadRequest;
    use crate::armor_viewer::load_model::load_model_scene;
    use crate::armor_viewer::model_refs::HardpointTarget;
    use crate::viewport::types::Vec3;
    use crate::viewport::types::ViewRect;

    /// Opens catalogued models from a real install, as the viewer opens them,
    /// and checks every vertex drawn is in view from several angles and each
    /// hardpoint marker is small beside the model. Run with `WOWS_DIR` set to
    /// an install; without one it returns at once.
    #[test]
    #[ignore = "needs a local game install in WOWS_DIR"]
    fn real_models_open_framed_whole_with_small_markers() {
        let Ok(wows_dir) = std::env::var("WOWS_DIR") else { return };
        let wows_dir = std::path::PathBuf::from(wows_dir);
        let builds = wowsunpack::game_data::list_available_builds(&wows_dir).expect("installed builds");
        let build = *builds.last().expect("at least one build");
        let vfs = wowsunpack::game_data::build_game_vfs_for_build(
            &wows_dir,
            build,
            wowsunpack::game_data::AssetsBin::Overlay,
        )
        .expect("the game VFS");
        let metadata = Arc::new(GameMetadataProvider::from_vfs(&vfs).expect("GameParams"));
        let assets = ShipAssets::from_vfs_with_metadata(&vfs, Arc::clone(&metadata)).expect("ship assets");
        let library = assets.model_library().expect("the model library");
        let mount_armor = MountArmorIndex::build(&metadata, library.catalog());
        // This test opens no building.
        let buildings = wowsunpack::export::building_armor::BuildingArmorIndex::default();
        let models = ModelAssets { library: Arc::new(library), mount_armor, buildings };
        let entries = models.library.catalog().entries();
        let turrets = entries.iter().filter(|entry| entry.name() == "AGM034_16in50_Mk7");
        let directors = entries.iter().filter(|entry| entry.name().contains("Director")).take(3);
        let paths: Vec<_> = turrets.chain(directors).map(|entry| entry.path().clone()).collect();
        assert_eq!(paths.len(), 4, "the turret and three directors are catalogued");

        let mut marked = 0;
        for path in paths {
            let cancel = crate::armor_viewer::scene_load::LoadCancel::new();
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
                .map(|(transform, p)| {
                    Vec3::from(transform.map_or(p, |t| wowsunpack::export::armor_ray::transform_point(t, p)))
                })
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
