//! Annotation tool interaction, selection, and movement.
//!
//! Moved from the desktop `minimap_view/shapes.rs` to share between desktop
//! and web clients. All functions use shared egui types only.

use egui::Color32;
use egui::Vec2;

use crate::rendering::ROTATION_HANDLE_RADIUS;
use crate::rendering::annotation_distance;
use crate::rendering::annotation_screen_bounds;
use crate::rendering::minimap_vec2_to_screen;
use crate::rendering::rotation_handle_pos;
use crate::rendering::smooth_freehand;
use crate::transforms::MapTransform;
use crate::types::Annotation;
use crate::types::AnnotationState;
use crate::types::PaintTool;
use crate::types::ship_short_name;

// ─── Preset Colors ──────────────────────────────────────────────────────────

/// Preset colors for annotation color picker.
pub const PRESET_COLORS: &[(Color32, &str)] = &[
    (Color32::WHITE, "White"),
    (Color32::from_rgb(160, 160, 160), "Gray"),
    (Color32::from_rgb(230, 50, 50), "Red"),
    (Color32::from_rgb(240, 140, 30), "Orange"),
    (Color32::from_rgb(240, 230, 50), "Yellow"),
    (Color32::from_rgb(50, 200, 50), "Green"),
    (Color32::from_rgb(50, 120, 230), "Blue"),
    (Color32::from_rgb(180, 60, 230), "Purple"),
    (Color32::from_rgb(255, 130, 180), "Pink"),
];

// ─── Tool Interaction ───────────────────────────────────────────────────────

/// Result of processing an annotation tool interaction for one frame.
pub struct ToolInteractionResult {
    /// A new annotation to add.
    pub new_annotation: Option<Annotation>,
    /// Index of an annotation to erase (Eraser tool).
    pub erase_index: Option<usize>,
}

/// Process the active paint tool for one frame.
///
/// Returns any new annotation to add or erase index, but does NOT mutate the
/// annotation list — callers handle that plus collab sync.
pub fn handle_tool_interaction(
    ann: &mut AnnotationState,
    response: &egui::Response,
    transform: &MapTransform,
) -> ToolInteractionResult {
    let cursor_minimap = response.hover_pos().map(|p| transform.screen_to_minimap(p));
    let paint_color = ann.paint_color;
    let stroke_width = ann.stroke_width;
    let mut new_annotation: Option<Annotation> = None;
    let mut erase_index: Option<usize> = None;

    match &mut ann.active_tool {
        PaintTool::PlacingShip { species, friendly, yaw } => {
            if response.clicked()
                && let Some(pos) = cursor_minimap
            {
                new_annotation =
                    Some(Annotation::Ship { pos, yaw: *yaw, species: species.clone(), friendly: *friendly, config: None });
            }
        }
        PaintTool::Freehand { current_stroke } => {
            if response.dragged_by(egui::PointerButton::Primary)
                && let Some(pos) = cursor_minimap
            {
                current_stroke.get_or_insert_with(Vec::new).push(pos);
            }
            if response.drag_stopped_by(egui::PointerButton::Primary)
                && let Some(points) = current_stroke.take()
                && points.len() >= 2
            {
                let points = smooth_freehand(points);
                new_annotation = Some(Annotation::FreehandStroke { points, color: paint_color, width: stroke_width });
            }
        }
        PaintTool::Eraser => {
            if response.clicked()
                && let Some(click_pos) = cursor_minimap
            {
                let threshold = 15.0;
                let mut closest_idx = None;
                let mut closest_dist = f32::MAX;
                for (i, a) in ann.annotations.iter().enumerate() {
                    let d = annotation_distance(a, click_pos);
                    if d < closest_dist {
                        closest_dist = d;
                        closest_idx = Some(i);
                    }
                }
                if closest_dist < threshold {
                    erase_index = closest_idx;
                }
            }
        }
        PaintTool::DrawingLine { start } => {
            if response.drag_started_by(egui::PointerButton::Primary)
                && let Some(pos) = cursor_minimap
            {
                *start = Some(pos);
            }
            if response.drag_stopped_by(egui::PointerButton::Primary)
                && let Some(s) = *start
            {
                if let Some(pos) = cursor_minimap
                    && (pos - s).length() > 1.0
                {
                    new_annotation =
                        Some(Annotation::Line { start: s, end: pos, color: paint_color, width: stroke_width });
                }
                *start = None;
            }
        }
        PaintTool::DrawingCircle { filled, center } => {
            if response.drag_started_by(egui::PointerButton::Primary)
                && let Some(pos) = cursor_minimap
            {
                *center = Some(pos);
            }
            if response.drag_stopped_by(egui::PointerButton::Primary)
                && let Some(origin) = *center
            {
                if let Some(pos) = cursor_minimap {
                    let radius = (pos - origin).length();
                    if radius > 1.0 {
                        new_annotation = Some(Annotation::Circle {
                            center: origin,
                            radius,
                            color: paint_color,
                            width: stroke_width,
                            filled: *filled,
                        });
                    }
                }
                *center = None;
            }
        }
        PaintTool::DrawingRect { filled, center } => {
            if response.drag_started_by(egui::PointerButton::Primary)
                && let Some(pos) = cursor_minimap
            {
                *center = Some(pos);
            }
            if response.drag_stopped_by(egui::PointerButton::Primary)
                && let Some(origin) = *center
            {
                if let Some(pos) = cursor_minimap {
                    let mid = (origin + pos) / 2.0;
                    let half = ((pos - origin) / 2.0).abs();
                    if half.x > 1.0 && half.y > 1.0 {
                        new_annotation = Some(Annotation::Rectangle {
                            center: mid,
                            half_size: half,
                            rotation: 0.0,
                            color: paint_color,
                            width: stroke_width,
                            filled: *filled,
                        });
                    }
                }
                *center = None;
            }
        }
        PaintTool::DrawingTriangle { filled, center } => {
            if response.drag_started_by(egui::PointerButton::Primary)
                && let Some(pos) = cursor_minimap
            {
                *center = Some(pos);
            }
            if response.drag_stopped_by(egui::PointerButton::Primary)
                && let Some(ctr) = *center
            {
                if let Some(pos) = cursor_minimap {
                    let radius = (pos - ctr).length();
                    if radius > 1.0 {
                        new_annotation = Some(Annotation::Triangle {
                            center: ctr,
                            radius,
                            rotation: 0.0,
                            color: paint_color,
                            width: stroke_width,
                            filled: *filled,
                        });
                    }
                }
                *center = None;
            }
        }
        PaintTool::DrawingArrow { current_stroke } => {
            let shift_held = response.ctx.input(|i| i.modifiers.shift);
            if response.dragged_by(egui::PointerButton::Primary)
                && let Some(pos) = cursor_minimap
            {
                let stroke = current_stroke.get_or_insert_with(Vec::new);
                if shift_held {
                    // Straight line: keep only start point + cursor
                    let start = stroke.first().copied().unwrap_or(pos);
                    stroke.clear();
                    stroke.push(start);
                    stroke.push(pos);
                } else {
                    stroke.push(pos);
                }
            }
            if response.drag_stopped_by(egui::PointerButton::Primary)
                && let Some(points) = current_stroke.take()
                && points.len() >= 2
            {
                // Smooth freehand arrows but not straight-line (shift) arrows (only 2 points).
                let points = if points.len() > 2 { smooth_freehand(points) } else { points };
                new_annotation = Some(Annotation::Arrow { points, color: paint_color, width: stroke_width });
            }
        }
        PaintTool::DrawingMeasurement { start } => {
            if response.drag_started_by(egui::PointerButton::Primary)
                && let Some(pos) = cursor_minimap
            {
                *start = Some(pos);
            }
            if response.drag_stopped_by(egui::PointerButton::Primary)
                && let Some(s) = *start
            {
                if let Some(pos) = cursor_minimap
                    && (pos - s).length() > 1.0
                {
                    new_annotation =
                        Some(Annotation::Measurement { start: s, end: pos, color: paint_color, width: stroke_width });
                }
                *start = None;
            }
        }
        PaintTool::None => {}
    }

    ToolInteractionResult { new_annotation, erase_index }
}

// ─── Select / Move / Rotate ─────────────────────────────────────────────────

/// Result of annotation selection/move/rotate for one frame.
pub struct SelectMoveResult {
    /// Indices of annotations that were moved (for collab sync on drag-stop).
    pub moved_indices: Vec<usize>,
    /// Index of annotation whose rotation drag just stopped.
    pub rotation_stopped_index: Option<usize>,
    /// Index of annotation that was selected by click.
    pub selected_by_click: bool,
}

/// Handle annotation select, move, and rotate when no drawing tool is active.
///
/// Manages rotation handle drag, click-to-select, and drag-to-move.
/// Returns info about what changed so callers can sync to collab.
pub fn handle_annotation_select_move(
    ann: &mut AnnotationState,
    response: &egui::Response,
    transform: &MapTransform,
) -> SelectMoveResult {
    let mut result =
        SelectMoveResult { moved_indices: Vec::new(), rotation_stopped_index: None, selected_by_click: false };

    // Check if drag started on the rotation handle or a measurement endpoint (single selection only)
    if response.drag_started_by(egui::PointerButton::Primary)
        && let Some(sel) = ann.single_selected()
        && sel < ann.annotations.len()
    {
        let has_rot = matches!(
            ann.annotations[sel],
            Annotation::Ship { .. } | Annotation::Rectangle { .. } | Annotation::Triangle { .. }
        );
        if has_rot && let Some(drag_origin) = response.interact_pointer_pos() {
            let (handle, _) = rotation_handle_pos(&ann.annotations[sel], transform);
            if (drag_origin - handle).length() < ROTATION_HANDLE_RADIUS + 8.0 {
                ann.dragging_rotation = true;
            }
        }
        // Check measurement endpoint
        if let Annotation::Measurement { start, end, .. } = &ann.annotations[sel]
            && let Some(drag_origin) = response.interact_pointer_pos()
        {
            let start_screen = minimap_vec2_to_screen(*start, transform);
            let end_screen = minimap_vec2_to_screen(*end, transform);
            let threshold = 15.0;
            if (drag_origin - start_screen).length() < threshold {
                ann.dragging_measurement_endpoint = Some(0);
            } else if (drag_origin - end_screen).length() < threshold {
                ann.dragging_measurement_endpoint = Some(1);
            }
        }
    }

    // Handle rotation drag (single selection only)
    if ann.dragging_rotation
        && response.dragged_by(egui::PointerButton::Primary)
        && let Some(sel) = ann.single_selected()
        && sel < ann.annotations.len()
        && let Some(cursor_screen) = response.hover_pos()
    {
        let center_screen = annotation_screen_bounds(&ann.annotations[sel], transform).center();
        let angle = -(cursor_screen.x - center_screen.x).atan2(-(cursor_screen.y - center_screen.y));
        match &mut ann.annotations[sel] {
            Annotation::Ship { yaw, .. } => *yaw = angle,
            Annotation::Rectangle { rotation, .. } => *rotation = angle,
            Annotation::Triangle { rotation, .. } => *rotation = angle,
            _ => {}
        }
    }

    // Stop rotation drag
    if ann.dragging_rotation && response.drag_stopped_by(egui::PointerButton::Primary) {
        ann.dragging_rotation = false;
        result.rotation_stopped_index = ann.single_selected();
    }

    // Click to select/deselect annotations
    if response.clicked()
        && let Some(click_pos) = response.hover_pos().map(|p| transform.screen_to_minimap(p))
    {
        let ctrl_held = response.ctx.input(|i| i.modifiers.command);
        let threshold = 15.0;
        let mut closest_idx = None;
        let mut closest_dist = f32::MAX;
        for (i, a) in ann.annotations.iter().enumerate() {
            let d = annotation_distance(a, click_pos);
            if d < closest_dist {
                closest_dist = d;
                closest_idx = Some(i);
            }
        }
        if closest_dist < threshold {
            if let Some(idx) = closest_idx {
                if ctrl_held {
                    // Toggle in/out of selection
                    if ann.selected_indices.contains(&idx) {
                        ann.selected_indices.remove(&idx);
                    } else {
                        ann.selected_indices.insert(idx);
                    }
                } else {
                    ann.select_single(idx);
                }
            }
        } else if !ctrl_held {
            ann.clear_selection();
        }
        result.selected_by_click = true;
    }

    // Drag to move selected annotations (only if not rotating)
    if !ann.dragging_rotation && response.dragged_by(egui::PointerButton::Primary) && ann.has_selection() {
        let delta = response.drag_delta();
        let minimap_delta = Vec2::new(
            delta.x / (transform.window_scale * transform.zoom),
            delta.y / (transform.window_scale * transform.zoom),
        );

        // Measurement endpoint drag: move only the dragged endpoint (single selection only)
        if let Some(ep) = ann.dragging_measurement_endpoint
            && let Some(sel) = ann.single_selected()
            && let Annotation::Measurement { start, end, .. } = &mut ann.annotations[sel]
        {
            if ep == 0 {
                *start += minimap_delta;
            } else {
                *end += minimap_delta;
            }
            result.moved_indices.push(sel);
        } else {
            let indices: Vec<usize> = ann.selected_indices.iter().copied().collect();
            for &sel in &indices {
                if sel < ann.annotations.len() {
                    move_annotation(&mut ann.annotations[sel], minimap_delta);
                }
            }
            result.moved_indices = indices;
        }
    }

    // Sync moved annotations on drag release
    if !ann.dragging_rotation && response.drag_stopped_by(egui::PointerButton::Primary) && ann.has_selection() {
        ann.dragging_measurement_endpoint = None;
        result.moved_indices = ann.selected_indices.iter().copied().collect();
    }

    result
}

/// Move an annotation by a delta in minimap coordinates.
pub fn move_annotation(ann: &mut Annotation, delta: Vec2) {
    match ann {
        Annotation::Ship { pos, .. } => *pos += delta,
        Annotation::FreehandStroke { points, .. } => {
            for p in points.iter_mut() {
                *p += delta;
            }
        }
        Annotation::Line { start, end, .. } => {
            *start += delta;
            *end += delta;
        }
        Annotation::Circle { center, .. } => *center += delta,
        Annotation::Rectangle { center, .. } => *center += delta,
        Annotation::Triangle { center, .. } => *center += delta,
        Annotation::Arrow { points, .. } => {
            for p in points.iter_mut() {
                *p += delta;
            }
        }
        Annotation::Measurement { start, end, .. } => {
            *start += delta;
            *end += delta;
        }
    }
}

// ─── Tool Helpers ───────────────────────────────────────────────────────────

/// Format the active tool as a human-readable label.
pub fn tool_label(tool: &PaintTool) -> Option<String> {
    match tool {
        PaintTool::None => None,
        PaintTool::PlacingShip { species, friendly, .. } => {
            let team = if *friendly { "Friendly" } else { "Enemy" };
            Some(format!("Placing {} {}", team, ship_short_name(species)))
        }
        PaintTool::Freehand { .. } => Some("Freehand".into()),
        PaintTool::Eraser => Some("Eraser".into()),
        PaintTool::DrawingLine { .. } => Some("Line".into()),
        PaintTool::DrawingCircle { .. } => Some("Circle".into()),
        PaintTool::DrawingRect { .. } => Some("Rectangle".into()),
        PaintTool::DrawingTriangle { .. } => Some("Triangle".into()),
        PaintTool::DrawingArrow { .. } => Some("Arrow".into()),
        PaintTool::DrawingMeasurement { .. } => Some("Measurement".into()),
    }
}

/// Determine the cursor icon for the current annotation tool state.
///
/// Returns `None` when no specific cursor should be set (caller may apply
/// zoom-dependent cursors).
pub fn annotation_cursor_icon(
    ann: &AnnotationState,
    response: &egui::Response,
    transform: &MapTransform,
) -> Option<egui::CursorIcon> {
    match &ann.active_tool {
        PaintTool::PlacingShip { .. } => Some(egui::CursorIcon::Cell),
        PaintTool::Freehand { .. }
        | PaintTool::Eraser
        | PaintTool::DrawingLine { .. }
        | PaintTool::DrawingCircle { .. }
        | PaintTool::DrawingRect { .. }
        | PaintTool::DrawingTriangle { .. }
        | PaintTool::DrawingArrow { .. }
        | PaintTool::DrawingMeasurement { .. } => Some(egui::CursorIcon::Crosshair),
        PaintTool::None => {
            if ann.has_selection() {
                if ann.dragging_rotation {
                    Some(egui::CursorIcon::Grabbing)
                } else if let Some(sel) = ann.single_selected()
                    && sel < ann.annotations.len()
                {
                    let has_rot = matches!(
                        ann.annotations[sel],
                        Annotation::Ship { .. } | Annotation::Rectangle { .. } | Annotation::Triangle { .. }
                    );
                    let on_handle = has_rot
                        && response.hover_pos().is_some_and(|hp| {
                            let (handle, _) = rotation_handle_pos(&ann.annotations[sel], transform);
                            (hp - handle).length() < ROTATION_HANDLE_RADIUS + 8.0
                        });
                    if on_handle { Some(egui::CursorIcon::Alias) } else { Some(egui::CursorIcon::Grab) }
                } else {
                    Some(egui::CursorIcon::Grab)
                }
            } else {
                None
            }
        }
    }
}

/// Handle scroll-wheel yaw rotation for the PlacingShip tool.
/// Returns `true` if the scroll was consumed (i.e. yaw was adjusted).
pub fn handle_scroll_yaw(ann: &mut AnnotationState, scroll_delta: f32) -> bool {
    if scroll_delta == 0.0 {
        return false;
    }
    match &mut ann.active_tool {
        PaintTool::PlacingShip { yaw, .. } => {
            *yaw += scroll_delta * 0.005;
            true
        }
        _ => false,
    }
}

// ─── Viewport Zoom / Pan ──────────────────────────────────────────────────

use crate::transforms::CanvasLayout;
use crate::transforms::ViewportZoomPan;
use wows_minimap_renderer::MINIMAP_SIZE;

/// Configuration for viewport zoom/pan behavior.
pub struct ZoomPanConfig {
    /// Whether left-click drag can pan (when no tool/selection is active).
    /// Desktop tactics/replay: `true`. Web: `false` (middle-drag only).
    pub allow_left_drag_pan: bool,
    /// HUD height in logical pixels. Affects cursor-centered zoom math.
    /// 0 for tactics boards; `HUD_HEIGHT` for replay/web replay views.
    pub hud_height: f32,
    /// Whether to route scroll events to tool yaw rotation when a PlacingShip
    /// tool is active. Desktop tactics/replay: `true`. Web: `false`.
    pub handle_tool_yaw: bool,
    /// Logical width of the zoomable map area. When set, pan clamping uses this
    /// instead of the full canvas width, preventing the map from panning into
    /// non-map areas (e.g. a stats panel). `None` = use full canvas width.
    pub map_width: Option<f32>,
}

/// Handle scroll-to-zoom, drag-to-pan, double-click-to-reset, and pan clamping.
///
/// `ctx` is used to read smooth scroll delta. The `left_pan_blocked` callback lets
/// callers veto left-click panning for context-specific reasons (e.g., cap point
/// dragging on the tactics board). Return `true` from the callback to block left-pan.
///
/// Returns `true` if the viewport was modified (caller should repaint).
#[allow(clippy::too_many_arguments)]
pub fn handle_viewport_zoom_pan(
    ctx: &egui::Context,
    response: &egui::Response,
    zoom_pan: &mut ViewportZoomPan,
    layout: &CanvasLayout,
    logical_canvas: Vec2,
    config: &ZoomPanConfig,
    mut annotation_state: Option<&mut AnnotationState>,
    left_pan_blocked: bool,
) -> bool {
    let mut changed = false;

    // Determine if an annotation tool is active
    let tool_active = annotation_state.as_ref().is_some_and(|a| !matches!(a.active_tool, PaintTool::None));

    // Scroll-wheel: zoom (or rotate when placing ship)
    if response.hovered() {
        let scroll_delta = ctx.input(|i| i.smooth_scroll_delta.y);
        if scroll_delta != 0.0 {
            let scroll_used_by_tool = config.handle_tool_yaw
                && tool_active
                && annotation_state.as_ref().is_some_and(|a| matches!(a.active_tool, PaintTool::PlacingShip { .. }));

            let scroll_used_by_tool = if scroll_used_by_tool {
                if let Some(ann) = annotation_state.as_deref_mut() {
                    handle_scroll_yaw(ann, scroll_delta)
                } else {
                    false
                }
            } else {
                false
            };

            if !scroll_used_by_tool {
                let zoom_speed = 0.01;
                let old_zoom = zoom_pan.zoom;
                let new_zoom = (old_zoom * (1.0 + scroll_delta * zoom_speed)).clamp(1.0, 10.0);

                if new_zoom != old_zoom {
                    // Cursor-centered zoom: keep the point under the cursor fixed
                    if let Some(cursor) = response.hover_pos() {
                        let local_x = (cursor.x - layout.origin.x) / layout.window_scale;
                        let local_y = (cursor.y - layout.origin.y) / layout.window_scale - config.hud_height;
                        let minimap_x = (local_x + zoom_pan.pan.x) / old_zoom;
                        let minimap_y = (local_y + zoom_pan.pan.y) / old_zoom;
                        zoom_pan.pan.x = minimap_x * new_zoom - local_x;
                        zoom_pan.pan.y = minimap_y * new_zoom - local_y;
                    }
                    zoom_pan.zoom = new_zoom;
                    changed = true;
                }
            }
        }
    }

    // Drag-to-pan: middle always pans; left only when allowed and nothing else consumes it
    let has_selection = annotation_state.as_ref().is_some_and(|a| a.has_selection());
    let left_pan = config.allow_left_drag_pan
        && !tool_active
        && !has_selection
        && !left_pan_blocked
        && response.dragged_by(egui::PointerButton::Primary);

    if response.dragged_by(egui::PointerButton::Middle) || left_pan {
        let delta = response.drag_delta();
        zoom_pan.pan.x -= delta.x / layout.window_scale;
        zoom_pan.pan.y -= delta.y / layout.window_scale;
        changed = true;
    }

    // Double-click to reset zoom/pan
    if response.double_clicked() {
        zoom_pan.zoom = 1.0;
        zoom_pan.pan = Vec2::ZERO;
        changed = true;
    }

    // Clamp pan so the map can't scroll past its edges.
    // When map_width is set, use it instead of the full canvas width so the map
    // can't pan into side-panel areas.
    let effective_w = config.map_width.unwrap_or(logical_canvas.x);
    let visible_w = (effective_w * layout.window_scale).min(layout.scaled_canvas.x) / layout.window_scale;
    let visible_h =
        (logical_canvas.y.min(layout.scaled_canvas.y) - config.hud_height * layout.window_scale) / layout.window_scale;
    let map_zoomed = MINIMAP_SIZE as f32 * zoom_pan.zoom;
    let max_pan_x = (map_zoomed - visible_w).max(0.0);
    let max_pan_y = (map_zoomed - visible_h).max(0.0);
    zoom_pan.pan.x = zoom_pan.pan.x.clamp(0.0, max_pan_x);
    zoom_pan.pan.y = zoom_pan.pan.y.clamp(0.0, max_pan_y);

    changed
}
