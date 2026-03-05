//! Annotation toolbar and context menu UI.
//!
//! Moved from the desktop `minimap_view/shapes.rs` to share between desktop
//! and web clients.

use std::collections::HashMap;

use egui::Color32;
use egui::Stroke;
use egui::TextureHandle;
use egui_phosphor::regular as icons;

use crate::interaction::PRESET_COLORS;
use crate::types::Annotation;
use crate::types::AnnotationState;
use crate::types::ENEMY_COLOR;
use crate::types::FRIENDLY_COLOR;
use crate::types::PaintTool;
use crate::types::SHIP_SPECIES;
use crate::types::ship_short_name;

// ─── Ship Species Buttons ───────────────────────────────────────────────────

/// Draw ship species buttons (friendly + enemy rows) in the annotation toolbar.
///
/// When icon textures are available, buttons show 24x24 rotated ship icons
/// tinted with the team colour; otherwise falls back to text abbreviations.
///
/// Modifies `ann.active_tool` when a button is clicked.
pub fn draw_ship_species_buttons(
    ui: &mut egui::Ui,
    ann: &mut AnnotationState,
    ship_icons: Option<&HashMap<String, TextureHandle>>,
) {
    ui.label(egui::RichText::new("Friendly Ships").color(FRIENDLY_COLOR).small());
    ui.horizontal(|ui| {
        for species in &SHIP_SPECIES {
            if ship_species_button(ui, species, FRIENDLY_COLOR, ship_icons) {
                ann.active_tool = PaintTool::PlacingShip { species: species.to_string(), friendly: true, yaw: 0.0 };
                ann.show_context_menu = false;
            }
        }
    });

    ui.label(egui::RichText::new("Enemy Ships").color(ENEMY_COLOR).small());
    ui.horizontal(|ui| {
        for species in &SHIP_SPECIES {
            if ship_species_button(ui, species, ENEMY_COLOR, ship_icons) {
                ann.active_tool = PaintTool::PlacingShip { species: species.to_string(), friendly: false, yaw: 0.0 };
                ann.show_context_menu = false;
            }
        }
    });
}

/// Single ship species button: icon if available, text fallback otherwise.
fn ship_species_button(
    ui: &mut egui::Ui,
    species: &str,
    tint: Color32,
    ship_icons: Option<&HashMap<String, TextureHandle>>,
) -> bool {
    if let Some(tex) = ship_icons.and_then(|icons| icons.get(species)) {
        let img = egui::Image::new(egui::load::SizedTexture::new(tex.id(), egui::vec2(24.0, 24.0)))
            .rotate(std::f32::consts::FRAC_PI_2, egui::vec2(0.5, 0.5))
            .tint(tint);
        ui.add(egui::Button::image(img)).on_hover_text(ship_short_name(species)).clicked()
    } else {
        ui.button(egui::RichText::new(ship_short_name(species)).color(tint)).clicked()
    }
}

// ─── Toolbar Group Helper ──────────────────────────────────────────────────

/// Draw a group of toolbar items that stay together when wrapping.
///
/// Uses the group's width from the previous frame (cached in egui temp storage)
/// to allocate a fixed-width sub-Ui via `allocate_ui`. This lets the outer
/// `horizontal_wrapped` see the correct size for wrapping decisions.
/// On the first frame (no cached width), uses `f32::INFINITY` so items get
/// full space; from the second frame onward the tight width is used.
fn toolbar_group(ui: &mut egui::Ui, id_salt: impl std::hash::Hash, add_contents: impl FnOnce(&mut egui::Ui)) {
    let id = ui.id().with(id_salt);
    let prev_width = ui.data(|d| d.get_temp::<f32>(id));
    let height = ui.spacing().interact_size.y;
    // Use cached width (with a small margin) so the outer wrapping layout
    // sees the real size. First frame: use INFINITY so nothing is clipped.
    let alloc_width = prev_width.map_or(f32::INFINITY, |w| w + 1.0);
    let r = ui.allocate_ui(egui::vec2(alloc_width, height), |ui| ui.horizontal(|ui| add_contents(ui)));
    // Cache the inner horizontal's actual width for next frame.
    let inner_width = r.inner.response.rect.width();
    ui.data_mut(|d| d.insert_temp(id, inner_width));
}

// ─── Annotation Toolbar (always-visible) ───────────────────────────────────

/// Result of drawing the annotation toolbar or context menu.
pub struct ToolbarResult {
    /// `true` if the undo button was clicked (caller may need to sync collab).
    pub did_undo: bool,
    /// `true` if the "Clear All" button was clicked (caller may need to sync collab).
    pub did_clear: bool,
}

/// Always-visible horizontal icon toolbar for annotation tools.
///
/// Contains: select tool, ship placement popups, drawing tools, eraser,
/// color picker + presets, stroke width, undo/clear.
///
/// When `locked` is `true`, all interactive elements are disabled and a lock
/// icon is shown.
pub fn draw_annotation_toolbar(
    ui: &mut egui::Ui,
    ann: &mut AnnotationState,
    ship_icons: Option<&HashMap<String, TextureHandle>>,
    locked: bool,
) -> ToolbarResult {
    let mut did_undo = false;
    let mut did_clear = false;

    // Snapshot tool state upfront (avoids borrow issues with &ann.active_tool).
    let is_none = matches!(ann.active_tool, PaintTool::None);
    let is_arrow = matches!(ann.active_tool, PaintTool::DrawingArrow { .. });
    let is_freehand = matches!(ann.active_tool, PaintTool::Freehand { .. });
    let is_line = matches!(ann.active_tool, PaintTool::DrawingLine { .. });
    let is_circle = matches!(ann.active_tool, PaintTool::DrawingCircle { .. });
    let circle_filled = matches!(ann.active_tool, PaintTool::DrawingCircle { filled: true, .. });
    let is_rect = matches!(ann.active_tool, PaintTool::DrawingRect { .. });
    let rect_filled = matches!(ann.active_tool, PaintTool::DrawingRect { filled: true, .. });
    let is_triangle = matches!(ann.active_tool, PaintTool::DrawingTriangle { .. });
    let triangle_filled = matches!(ann.active_tool, PaintTool::DrawingTriangle { filled: true, .. });
    let is_measure = matches!(ann.active_tool, PaintTool::DrawingMeasurement { .. });
    let is_eraser = matches!(ann.active_tool, PaintTool::Eraser);

    ui.horizontal_wrapped(|ui| {
        if locked {
            ui.disable();
        }
        {
            // ── Select ──
            toolbar_group(ui, "select", |ui| {
                if ui.selectable_label(is_none, icons::CURSOR).on_hover_text("Select").clicked() {
                    ann.active_tool = PaintTool::None;
                }
            });

            ui.separator();

            // ── Ship placement popups ──
            toolbar_group(ui, "ships", |ui| {
                let friendly_btn = ui
                    .button(egui::RichText::new(icons::ANCHOR).color(FRIENDLY_COLOR))
                    .on_hover_text("Place friendly ship");
                egui::Popup::from_toggle_button_response(&friendly_btn)
                    .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                    .show(|ui| {
                        ui.label(egui::RichText::new("Friendly Ships").color(FRIENDLY_COLOR).small());
                        ui.horizontal(|ui| {
                            for species in &SHIP_SPECIES {
                                if ship_species_button(ui, species, FRIENDLY_COLOR, ship_icons) {
                                    ann.active_tool = PaintTool::PlacingShip {
                                        species: species.to_string(),
                                        friendly: true,
                                        yaw: 0.0,
                                    };
                                }
                            }
                        });
                    });

                let enemy_btn =
                    ui.button(egui::RichText::new(icons::ANCHOR).color(ENEMY_COLOR)).on_hover_text("Place enemy ship");
                egui::Popup::from_toggle_button_response(&enemy_btn)
                    .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                    .show(|ui| {
                        ui.label(egui::RichText::new("Enemy Ships").color(ENEMY_COLOR).small());
                        ui.horizontal(|ui| {
                            for species in &SHIP_SPECIES {
                                if ship_species_button(ui, species, ENEMY_COLOR, ship_icons) {
                                    ann.active_tool = PaintTool::PlacingShip {
                                        species: species.to_string(),
                                        friendly: false,
                                        yaw: 0.0,
                                    };
                                }
                            }
                        });
                    });
            });

            ui.separator();

            // ── Drawing tools ──
            toolbar_group(ui, "draw_tools", |ui| {
                if ui.selectable_label(is_arrow, icons::ARROW_BEND_UP_RIGHT).on_hover_text("Arrow").clicked() {
                    ann.active_tool = PaintTool::DrawingArrow { current_stroke: None };
                }
                if ui.selectable_label(is_freehand, icons::PAINT_BRUSH).on_hover_text("Freehand").clicked() {
                    ann.active_tool = PaintTool::Freehand { current_stroke: None };
                }
                if ui.selectable_label(is_line, icons::LINE_SEGMENT).on_hover_text("Line").clicked() {
                    ann.active_tool = PaintTool::DrawingLine { start: None };
                }
                {
                    let hover = if is_circle { "Circle (click to toggle fill)" } else { "Circle" };
                    if ui.selectable_label(is_circle, icons::CIRCLE).on_hover_text(hover).clicked() {
                        if is_circle {
                            ann.active_tool = PaintTool::DrawingCircle { filled: !circle_filled, center: None };
                        } else {
                            ann.active_tool = PaintTool::DrawingCircle { filled: false, center: None };
                        }
                    }
                }
                {
                    let hover = if is_rect { "Rectangle (click to toggle fill)" } else { "Rectangle" };
                    if ui.selectable_label(is_rect, icons::SQUARE).on_hover_text(hover).clicked() {
                        if is_rect {
                            ann.active_tool = PaintTool::DrawingRect { filled: !rect_filled, center: None };
                        } else {
                            ann.active_tool = PaintTool::DrawingRect { filled: false, center: None };
                        }
                    }
                }
                {
                    let hover = if is_triangle { "Triangle (click to toggle fill)" } else { "Triangle" };
                    if ui.selectable_label(is_triangle, icons::TRIANGLE).on_hover_text(hover).clicked() {
                        if is_triangle {
                            ann.active_tool = PaintTool::DrawingTriangle { filled: !triangle_filled, center: None };
                        } else {
                            ann.active_tool = PaintTool::DrawingTriangle { filled: false, center: None };
                        }
                    }
                }
                if ui.selectable_label(is_measure, icons::RULER).on_hover_text("Measurement").clicked() {
                    ann.active_tool = PaintTool::DrawingMeasurement { start: None };
                }
            });

            ui.separator();

            toolbar_group(ui, "eraser", |ui| {
                if ui.selectable_label(is_eraser, icons::ERASER).on_hover_text("Eraser").clicked() {
                    ann.active_tool = PaintTool::Eraser;
                }
            });

            ui.separator();

            // ── Color picker + presets ──
            toolbar_group(ui, "colors", |ui| {
                egui::color_picker::color_edit_button_srgba(
                    ui,
                    &mut ann.paint_color,
                    egui::color_picker::Alpha::Opaque,
                );
                let swatch_size = egui::vec2(16.0, 16.0);
                for &(color, name) in PRESET_COLORS {
                    let selected = ann.paint_color == color;
                    let (rect, resp) = ui.allocate_exact_size(swatch_size, egui::Sense::click());
                    ui.painter().rect_filled(rect, egui::CornerRadius::same(3), color);
                    if selected {
                        ui.painter().rect_stroke(
                            rect,
                            egui::CornerRadius::same(3),
                            Stroke::new(2.0, Color32::WHITE),
                            egui::StrokeKind::Outside,
                        );
                    }
                    if resp.on_hover_text(name).clicked() {
                        ann.paint_color = color;
                    }
                }
            });

            ui.separator();

            // ── Stroke width ──
            toolbar_group(ui, "stroke", |ui| {
                ui.label(egui::RichText::new(icons::LINE_SEGMENT).weak());
                ui.add(egui::Slider::new(&mut ann.stroke_width, 1.0..=8.0).max_decimals(1).show_value(false))
                    .on_hover_text(format!("Stroke width: {:.1}", ann.stroke_width));
            });

            ui.separator();

            // ── Undo / Clear ──
            toolbar_group(ui, "undo_clear", |ui| {
                if ui.button(icons::ARROW_COUNTER_CLOCKWISE).on_hover_text("Undo").clicked() {
                    ann.undo();
                    did_undo = true;
                }
                if ui
                    .button(egui::RichText::new(icons::TRASH).color(Color32::from_rgb(255, 100, 100)))
                    .on_hover_text("Clear All")
                    .clicked()
                {
                    ann.save_undo();
                    ann.annotations.clear();
                    ann.annotation_ids.clear();
                    ann.annotation_owners.clear();
                    ann.clear_selection();
                    did_clear = true;
                }
            });
        }

        if locked {
            // Lock icon should remain visible even though the rest is disabled.
            // Use add_enabled() so this single widget is interactive.
            ui.separator();
            ui.add_enabled(true, egui::Label::new(egui::RichText::new(icons::LOCK).color(Color32::YELLOW)))
                .on_hover_text("Annotations locked by host");
        }
    });

    ToolbarResult { did_undo, did_clear }
}

// ─── Annotation Context Menu ───────────────────────────────────────────────

/// Result of drawing the common annotation context menu.
pub type AnnotationMenuResult = ToolbarResult;

/// Draw the common annotation context menu items: ship placement buttons,
/// drawing tool buttons, color presets, stroke width slider, and undo/clear.
///
/// Returns which actions were taken so the caller can send collab events.
pub fn draw_annotation_menu_common(
    ui: &mut egui::Ui,
    ann: &mut AnnotationState,
    ship_icons: Option<&HashMap<String, TextureHandle>>,
) -> AnnotationMenuResult {
    draw_ship_species_buttons(ui, ann, ship_icons);

    ui.separator();

    // ── Drawing tools row ──
    ui.label(egui::RichText::new("Drawing Tools").small());
    ui.horizontal(|ui| {
        if ui.button(icons::ARROW_BEND_UP_RIGHT).on_hover_text("Arrow (Ctrl+1)").clicked() {
            ann.active_tool = PaintTool::DrawingArrow { current_stroke: None };
            ann.show_context_menu = false;
        }
        if ui.button(icons::PAINT_BRUSH).on_hover_text("Freehand (Ctrl+2)").clicked() {
            ann.active_tool = PaintTool::Freehand { current_stroke: None };
            ann.show_context_menu = false;
        }
        if ui.button(icons::ERASER).on_hover_text("Eraser (Ctrl+3)").clicked() {
            ann.active_tool = PaintTool::Eraser;
            ann.show_context_menu = false;
        }
        if ui.button(icons::LINE_SEGMENT).on_hover_text("Line (Ctrl+4)").clicked() {
            ann.active_tool = PaintTool::DrawingLine { start: None };
            ann.show_context_menu = false;
        }
        if ui.button(icons::CIRCLE).on_hover_text("Circle (Ctrl+5)").clicked() {
            ann.active_tool = PaintTool::DrawingCircle { filled: false, center: None };
            ann.show_context_menu = false;
        }
        if ui.button(icons::SQUARE).on_hover_text("Rectangle (Ctrl+6)").clicked() {
            ann.active_tool = PaintTool::DrawingRect { filled: false, center: None };
            ann.show_context_menu = false;
        }
        if ui.button(icons::TRIANGLE).on_hover_text("Triangle (Ctrl+7)").clicked() {
            ann.active_tool = PaintTool::DrawingTriangle { filled: false, center: None };
            ann.show_context_menu = false;
        }
        if ui.button(icons::RULER).on_hover_text("Measurement (Ctrl+M)").clicked() {
            ann.active_tool = PaintTool::DrawingMeasurement { start: None };
            ann.show_context_menu = false;
        }
    });

    ui.separator();

    // ── Color presets + stroke width ──
    ui.horizontal(|ui| {
        let swatch_size = egui::vec2(16.0, 16.0);
        egui::color_picker::color_edit_button_srgba(ui, &mut ann.paint_color, egui::color_picker::Alpha::Opaque);
        ui.add_space(4.0);
        for &(color, name) in PRESET_COLORS {
            let selected = ann.paint_color == color;
            let (rect, resp) = ui.allocate_exact_size(swatch_size, egui::Sense::click());
            ui.painter().rect_filled(rect, egui::CornerRadius::same(3), color);
            if selected {
                ui.painter().rect_stroke(
                    rect,
                    egui::CornerRadius::same(3),
                    Stroke::new(2.0, Color32::WHITE),
                    egui::StrokeKind::Outside,
                );
            }
            if resp.on_hover_text(name).clicked() {
                ann.paint_color = color;
            }
        }
    });
    ui.horizontal(|ui| {
        ui.label("Width:");
        ui.add(egui::Slider::new(&mut ann.stroke_width, 1.0..=8.0).max_decimals(1));
    });

    ui.separator();

    // ── Undo / Clear ──
    let mut did_undo = false;
    let mut did_clear = false;
    ui.horizontal(|ui| {
        if ui.button("Undo").clicked() {
            ann.undo();
            ann.show_context_menu = false;
            did_undo = true;
        }
        if ui.button("Clear All").clicked() {
            ann.save_undo();
            ann.annotations.clear();
            ann.annotation_ids.clear();
            ann.annotation_owners.clear();
            ann.clear_selection();
            ann.show_context_menu = false;
            did_clear = true;
        }
    });
    AnnotationMenuResult { did_undo, did_clear }
}

// ─── Edit Popup Action ──────────────────────────────────────────────────────

/// Actions returned by the annotation edit popup for the caller to execute.
pub enum EditPopupAction {
    /// The annotation at the given index was modified in-place (size, color, filled, team toggle).
    /// Caller should sync the updated annotation to collab.
    Updated,
    /// The annotation at the given index should be deleted.
    /// Caller should remove it and sync to collab.
    Deleted { id: u64 },
}

/// Draw the annotation selection edit popup (size, color, filled, team, delete).
///
/// Returns actions for the caller to apply (collab sync, deletion, etc.).
/// `map_space_size` is used for km-based circle size display (pass None to skip).
pub fn draw_annotation_edit_popup(
    ctx: &egui::Context,
    area_id: egui::Id,
    ann: &mut AnnotationState,
    sel_idx: usize,
    bounds: egui::Rect,
    map_space_size: Option<f32>,
) -> Vec<EditPopupAction> {
    let mut actions = Vec::new();

    let popup_pos = egui::Pos2::new(bounds.right() + 8.0, bounds.center().y);
    egui::Area::new(area_id).order(egui::Order::Foreground).fixed_pos(popup_pos).interactable(true).show(ctx, |ui| {
        let frame = egui::Frame::NONE
            .fill(Color32::from_gray(30))
            .corner_radius(egui::CornerRadius::same(6))
            .inner_margin(egui::Margin::same(6))
            .stroke(Stroke::new(1.0, Color32::from_gray(80)));
        frame.show(ui, |ui| {
            if sel_idx >= ann.annotations.len() {
                return;
            }

            // Size slider (for circle, rect, triangle)
            let has_size = matches!(
                ann.annotations[sel_idx],
                Annotation::Circle { .. } | Annotation::Rectangle { .. } | Annotation::Triangle { .. }
            );
            if has_size {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Size").small());
                    let mut size = match &ann.annotations[sel_idx] {
                        Annotation::Circle { radius, .. } => *radius,
                        Annotation::Rectangle { half_size, .. } => (half_size.x + half_size.y) / 2.0,
                        Annotation::Triangle { radius, .. } => *radius,
                        _ => 0.0,
                    };
                    let old = size;
                    let is_circle = matches!(&ann.annotations[sel_idx], Annotation::Circle { .. });
                    let use_km = is_circle && map_space_size.is_some();
                    if use_km {
                        // Approximate km conversion: minimap pixels → world units → km
                        let space_size = map_space_size.unwrap();
                        let bw = size / 768.0 * space_size;
                        let mut km = bw / 1000.0; // approximate: BigWorld units ≈ meters
                        let old_km = km;
                        ui.add(
                            egui::DragValue::new(&mut km).speed(0.1).range(0.1..=20.0).fixed_decimals(1).suffix(" km"),
                        );
                        if km != old_km {
                            size = km * 1000.0 / space_size * 768.0;
                        }
                    } else {
                        ui.add(egui::DragValue::new(&mut size).speed(1.0).range(5.0..=500.0));
                    }
                    if size != old && size > 0.0 {
                        match &mut ann.annotations[sel_idx] {
                            Annotation::Circle { radius, .. } => *radius = size,
                            Annotation::Rectangle { half_size, .. } => {
                                let ratio = if old > 0.0 { size / old } else { 1.0 };
                                *half_size *= ratio;
                            }
                            Annotation::Triangle { radius, .. } => *radius = size,
                            _ => {}
                        }
                        actions.push(EditPopupAction::Updated);
                    }
                });
            }

            // Color picker (for non-ship annotations)
            let is_ship = matches!(ann.annotations[sel_idx], Annotation::Ship { .. });
            if !is_ship {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Color").small());
                    let color_ref = match &mut ann.annotations[sel_idx] {
                        Annotation::FreehandStroke { color, .. } => color,
                        Annotation::Line { color, .. } => color,
                        Annotation::Circle { color, .. } => color,
                        Annotation::Rectangle { color, .. } => color,
                        Annotation::Triangle { color, .. } => color,
                        Annotation::Arrow { color, .. } => color,
                        Annotation::Measurement { color, .. } => color,
                        _ => unreachable!(),
                    };
                    let old_color = *color_ref;
                    egui::color_picker::color_edit_button_srgba(ui, color_ref, egui::color_picker::Alpha::Opaque);
                    if *color_ref != old_color {
                        actions.push(EditPopupAction::Updated);
                    }
                });
            }

            // Filled toggle (for circle, rect, triangle)
            let has_filled = matches!(
                ann.annotations[sel_idx],
                Annotation::Circle { .. } | Annotation::Rectangle { .. } | Annotation::Triangle { .. }
            );
            if has_filled {
                let filled_ref = match &mut ann.annotations[sel_idx] {
                    Annotation::Circle { filled, .. } => filled,
                    Annotation::Rectangle { filled, .. } => filled,
                    Annotation::Triangle { filled, .. } => filled,
                    _ => unreachable!(),
                };
                let old_filled = *filled_ref;
                ui.checkbox(filled_ref, egui::RichText::new("Filled").small());
                if *filled_ref != old_filled {
                    actions.push(EditPopupAction::Updated);
                }
            }

            // Team toggle (for ships)
            if is_ship && let Annotation::Ship { friendly, .. } = &mut ann.annotations[sel_idx] {
                let (label, color) = if *friendly { ("Friendly", FRIENDLY_COLOR) } else { ("Enemy  ", ENEMY_COLOR) };
                let btn =
                    egui::Button::new(egui::RichText::new(label).color(color).small()).min_size(egui::vec2(60.0, 0.0));
                if ui.add(btn).clicked() {
                    *friendly = !*friendly;
                    actions.push(EditPopupAction::Updated);
                }
            }

            // Delete button
            if ui
                .button(egui::RichText::new(icons::TRASH).color(Color32::from_rgb(255, 100, 100)))
                .on_hover_text("Delete")
                .clicked()
            {
                ann.save_undo();
                let id = ann.annotation_ids[sel_idx];
                ann.annotations.remove(sel_idx);
                ann.annotation_ids.remove(sel_idx);
                ann.annotation_owners.remove(sel_idx);
                ann.clear_selection();
                actions.push(EditPopupAction::Deleted { id });
            }
        });
    });

    actions
}
