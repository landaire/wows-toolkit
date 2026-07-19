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

use crate::armor_viewer::load_ship::ArmorTriangleTooltip;
use crate::armor_viewer::load_ship::LoadedShipArmor;
use crate::armor_viewer::load_ship::PlateKey;
use crate::armor_viewer::picking_ui;
use crate::armor_viewer::upload;
use crate::armor_viewer::upload::upload_armor_to_viewport;
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
    /// `plate_visibility` change can re-upload without reloading the ship.
    current_armor: Option<Arc<LoadedShipArmor>>,
    /// Explicitly-hidden plates (click-to-hide / context-menu toggle,
    /// `picking_ui.rs`); absent = visible. Reset whenever a new ship loads.
    /// Mutations always go through `toggle_plate`/`set_plate_hidden`/
    /// `show_all_plates`/`disable_material` -- TODO(Task 7): undo/redo will
    /// wrap those with a `VisibilityUndoStack` snapshot.
    plate_visibility: HashMap<PlateKey, bool>,
    /// The currently hovered armor plate from CPU picking, if any.
    hovered: Option<HoverInfo>,
    /// The overlay mesh highlighting `hovered`'s plate, if any, so it can be
    /// removed/replaced when the hovered plate changes.
    hover_highlight: Option<(PlateKey, MeshId)>,
    /// Per-mesh triangle tooltip data from the most recent armor upload, used
    /// to map a `pick()` hit back to its `ArmorTriangleTooltip`.
    mesh_triangle_info: Vec<(MeshId, Vec<ArmorTriangleTooltip>)>,
}

impl ViewportView {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();
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
            plate_visibility: HashMap::new(),
            hovered: None,
            hover_highlight: None,
            mesh_triangle_info: Vec::new(),
        };
        this.start_gpu_init(cx);
        this
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
    /// later `plate_visibility`-only re-uploads (`reupload_current_armor`).
    /// Resets `plate_visibility` and any hover state -- a freshly loaded ship
    /// starts with every plate visible and nothing hovered, matching the
    /// egui app's own reset on ship load (`tab.rs:1453`, `2393-2394`).
    /// Callers must have already checked `self.gpu` is `Ready`; a no-op
    /// otherwise.
    fn upload_armor_now(&mut self, armor: Arc<LoadedShipArmor>) {
        let GpuState::Ready { ctx, .. } = &self.gpu else { return };
        let device = ctx.device.clone();
        self.plate_visibility.clear();
        self.hovered = None;
        self.hover_highlight = None;
        self.mesh_triangle_info = upload_armor_to_viewport(&mut self.viewport, &device, &armor, &self.plate_visibility);
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

    /// WASD moves the camera target; arrows move it vertically / orbit the
    /// azimuth. Tracks the key as held and (re)starts the ~60Hz movement
    /// ticker (`start_key_ticker`), which applies the per-tick camera delta
    /// every tick for smooth continuous motion with no OS auto-repeat delay,
    /// for as long as any movement key remains held.
    fn handle_key_down(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if !self.is_gpu_ready() {
            return;
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
        let mesh_id =
            picking_ui::upload_plate_highlight(&mut self.viewport, &ctx.device, &armor, key, &self.plate_visibility);
        self.hover_highlight = Some((key.clone(), mesh_id));
        self.viewport.mark_dirty();
    }

    /// Toggles a single plate's visibility and re-uploads. The single seam
    /// every visibility mutation goes through (along with `show_all_plates`/
    /// `disable_material`) -- TODO(Task 7): undo/redo will snapshot
    /// `plate_visibility` around this call.
    fn toggle_plate(&mut self, key: PlateKey, cx: &mut Context<Self>) {
        let hidden = self.plate_visibility.get(&key).copied().unwrap_or(false);
        self.set_plate_hidden(key, !hidden, cx);
    }

    /// Sets one plate's hidden state. `plate_visibility` stores only
    /// explicitly-hidden keys (absent = visible), so showing a plate removes
    /// its entry rather than inserting `false`.
    fn set_plate_hidden(&mut self, key: PlateKey, hidden: bool, cx: &mut Context<Self>) {
        if hidden {
            self.plate_visibility.insert(key, true);
        } else {
            self.plate_visibility.remove(&key);
        }
        self.reupload_current_armor(cx);
    }

    /// Clears every explicitly-hidden plate ("Show all hidden plates" context
    /// menu action, `tab.rs:5393-5402`).
    fn show_all_plates(&mut self, cx: &mut Context<Self>) {
        if self.plate_visibility.is_empty() {
            return;
        }
        self.plate_visibility.clear();
        self.reupload_current_armor(cx);
    }

    /// Hides every plate thickness belonging to `(zone, material)` ("Disable
    /// {material}" context menu action, `tab.rs:5406-5414`). The egui app
    /// tracks this via a separate `part_visibility` map (Milestone 4's part
    /// list); this port has no part-level state yet, so it reproduces the
    /// same *effect* by hiding each of that part's known plate thicknesses
    /// individually (`LoadedShipArmor::zone_part_plates`).
    fn disable_material(&mut self, zone: &str, material: &str, cx: &mut Context<Self>) {
        let Some(armor) = &self.current_armor else { return };
        let thicknesses: Vec<i32> = armor
            .zone_part_plates
            .iter()
            .find(|z| z.name == zone)
            .and_then(|z| z.parts.iter().find(|p| p.name == material))
            .map(|p| p.plates.clone())
            .unwrap_or_default();
        if thicknesses.is_empty() {
            return;
        }
        for thickness in thicknesses {
            self.plate_visibility.insert((zone.to_string(), material.to_string(), thickness), true);
        }
        self.reupload_current_armor(cx);
    }

    /// Re-uploads `current_armor` honoring the current `plate_visibility`,
    /// without moving the camera (`upload::reupload_armor_plates`). If the
    /// hovered plate is still visible its highlight is rebuilt against the
    /// fresh geometry; if it just became hidden the hover/tooltip are cleared
    /// outright rather than waiting for the next mouse-move.
    fn reupload_current_armor(&mut self, cx: &mut Context<Self>) {
        let GpuState::Ready { ctx, .. } = &self.gpu else { return };
        let Some(armor) = self.current_armor.clone() else { return };
        let device = ctx.device.clone();
        self.mesh_triangle_info =
            upload::reupload_armor_plates(&mut self.viewport, &device, &armor, &self.plate_visibility);
        // `viewport.clear()` (inside the re-upload) already dropped the old
        // highlight mesh; forget the stale id before rebuilding against the
        // fresh geometry.
        self.hover_highlight = None;
        if let Some(hover) = self.hovered.clone() {
            let hidden = self.plate_visibility.get(&hover.key).copied().unwrap_or(false);
            if hidden {
                self.hovered = None;
            } else {
                self.rebuild_hover_highlight(&hover.key);
            }
        }
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
                .with_priority(2)
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
            })
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
