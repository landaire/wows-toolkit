//! Annotation rendering, hit testing, and geometry helpers.
//!
//! These functions are used by both the replay renderer and the tactics board.
//! Ship icon textures are optional — when absent, ships are rendered as simple
//! colored circles.
//!
//! Core rendering, hit testing, geometry, tool interaction, toolbar, and
//! selection/move functions are provided by `wt_collab_egui` (shared with the
//! WASM web client) and re-exported here. Desktop-only UI code (shortcut
//! overlay, edit popup with km conversion, font registration) lives in this
//! module.

use std::sync::Arc;
use std::sync::mpsc;

use egui::Color32;
use egui::Pos2;
use egui::Rect;
use egui::Stroke;
use parking_lot::Mutex;

use crate::collab::peer::LocalEvent;
use wowsunpack::game_params::types::BigWorldDistance;
use wowsunpack::game_params::types::Km;

use super::Annotation;
use super::AnnotationState;
use super::ENEMY_COLOR;
use super::FRIENDLY_COLOR;
use super::PaintTool;
use super::send_annotation_remove;
use super::send_annotation_update;

// Re-export shared rendering items from wt-collab-egui.
pub use wt_collab_egui::rendering::PING_DURATION;
pub use wt_collab_egui::rendering::annotation_screen_bounds;
pub use wt_collab_egui::rendering::draw_grid;
pub use wt_collab_egui::rendering::draw_map_background;
pub use wt_collab_egui::rendering::draw_pings;
pub use wt_collab_egui::rendering::draw_remote_cursors;
pub use wt_collab_egui::rendering::game_font;
pub use wt_collab_egui::rendering::render_annotation;
pub use wt_collab_egui::rendering::render_cap_point;
pub use wt_collab_egui::rendering::render_selection_highlight;
pub use wt_collab_egui::rendering::render_tool_preview;
pub use wt_collab_egui::types::CapPointView;
pub use wt_collab_egui::types::GridStyle;
pub use wt_collab_egui::types::MapPing;

// Re-export shared transform items from wt-collab-egui.
pub use wt_collab_egui::transforms::compute_canvas_layout;
pub use wt_collab_egui::transforms::compute_map_clip_rect;

// Re-export shared tool interaction items from wt-collab-egui.
pub use wt_collab_egui::interaction::ZoomPanConfig;
pub use wt_collab_egui::interaction::annotation_cursor_icon;
pub use wt_collab_egui::interaction::handle_annotation_select_move;
pub use wt_collab_egui::interaction::handle_tool_interaction;
pub use wt_collab_egui::interaction::handle_viewport_zoom_pan;
pub use wt_collab_egui::interaction::tool_label;

/// Add the `GameFont` family to `font_defs`.
///
/// If `game_fonts` is `Some`, the real Warhelios Bold + CJK fallback fonts are
/// inserted. Otherwise `GameFont` is aliased to the default proportional family
/// so that [`game_font`] never panics.
///
/// The caller is responsible for passing the result to `ctx.set_fonts()`.
pub fn register_game_fonts(
    font_defs: &mut egui::FontDefinitions,
    game_fonts: Option<&wows_minimap_renderer::GameFonts>,
) {
    if let Some(fonts) = game_fonts {
        font_defs
            .font_data
            .insert("game_font_primary".to_owned(), egui::FontData::from_owned(fonts.primary_bytes.clone()).into());
        let mut family_fonts = vec!["game_font_primary".to_owned()];
        let fallback_names = ["game_font_ko", "game_font_jp", "game_font_cn"];
        for (i, bytes) in fonts.fallback_bytes.iter().enumerate() {
            let name = fallback_names.get(i).unwrap_or(&"game_font_fallback").to_string();
            font_defs.font_data.insert(name.clone(), egui::FontData::from_owned(bytes.clone()).into());
            family_fonts.push(name);
        }
        font_defs.families.insert(egui::FontFamily::Name("GameFont".into()), family_fonts);
    } else if !font_defs.families.contains_key(&egui::FontFamily::Name("GameFont".into())) {
        let proportional = font_defs.families.get(&egui::FontFamily::Proportional).cloned().unwrap_or_default();
        font_defs.families.insert(egui::FontFamily::Name("GameFont".into()), proportional);
    }
}

/// Handle keyboard shortcuts for switching annotation tools.
///
/// Reads `Ctrl+1..7` and `Ctrl+M` from the egui context input. Sets the
/// active tool and clears selection when a shortcut is pressed.
/// Returns `true` if a shortcut was consumed.
pub fn handle_tool_shortcuts(ctx: &egui::Context, ann: &mut AnnotationState) -> bool {
    let consumed = ctx.input(|i| {
        if !i.modifiers.command {
            return None;
        }
        if i.key_pressed(egui::Key::Num1) {
            Some(PaintTool::DrawingArrow { current_stroke: None })
        } else if i.key_pressed(egui::Key::Num2) {
            Some(PaintTool::Freehand { current_stroke: None })
        } else if i.key_pressed(egui::Key::Num3) {
            Some(PaintTool::Eraser)
        } else if i.key_pressed(egui::Key::Num4) {
            Some(PaintTool::DrawingLine { start: None })
        } else if i.key_pressed(egui::Key::Num5) {
            Some(PaintTool::DrawingCircle { filled: false, center: None })
        } else if i.key_pressed(egui::Key::Num6) {
            Some(PaintTool::DrawingRect { filled: false, center: None })
        } else if i.key_pressed(egui::Key::Num7) {
            Some(PaintTool::DrawingTriangle { filled: false, center: None })
        } else if i.key_pressed(egui::Key::M) {
            Some(PaintTool::DrawingMeasurement { start: None })
        } else {
            None
        }
    });
    if let Some(tool) = consumed {
        ann.active_tool = tool;
        ann.clear_selection();
        true
    } else {
        false
    }
}

/// Draw a shortcut overlay visible while Ctrl is held.
///
/// Shows a semi-transparent panel in the bottom-right listing all tool
/// shortcuts, undo, multi-select, stroke width, and delete keys.
pub fn draw_shortcut_overlay(ctx: &egui::Context, area_id: egui::Id) {
    if !ctx.input(|i| i.modifiers.command) {
        return;
    }
    egui::Area::new(area_id)
        .order(egui::Order::Tooltip)
        .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-8.0, -8.0))
        .interactable(false)
        .show(ctx, |ui| {
            let frame = egui::Frame::NONE
                .fill(Color32::from_rgba_unmultiplied(20, 20, 20, 200))
                .corner_radius(egui::CornerRadius::same(6))
                .inner_margin(egui::Margin::same(8));
            frame.show(ui, |ui| {
                let s = |text: &str| egui::RichText::new(text).size(11.0).color(Color32::from_gray(220));
                let dim = |text: &str| egui::RichText::new(text).size(11.0).color(Color32::from_gray(130));
                ui.label(s("Keyboard Shortcuts"));
                ui.separator();
                ui.label(dim("Tools"));
                egui::Grid::new(area_id.with("grid")).num_columns(2).spacing([12.0, 2.0]).show(ui, |ui| {
                    ui.label(s("Ctrl+1"));
                    ui.label(s("Arrow"));
                    ui.end_row();
                    ui.label(s("Ctrl+2"));
                    ui.label(s("Freehand"));
                    ui.end_row();
                    ui.label(s("Ctrl+3"));
                    ui.label(s("Eraser"));
                    ui.end_row();
                    ui.label(s("Ctrl+4"));
                    ui.label(s("Line"));
                    ui.end_row();
                    ui.label(s("Ctrl+5"));
                    ui.label(s("Circle"));
                    ui.end_row();
                    ui.label(s("Ctrl+6"));
                    ui.label(s("Rectangle"));
                    ui.end_row();
                    ui.label(s("Ctrl+7"));
                    ui.label(s("Triangle"));
                    ui.end_row();
                    ui.label(s("Ctrl+M"));
                    ui.label(s("Measurement"));
                    ui.end_row();
                });
                ui.add_space(2.0);
                ui.label(dim("Actions"));
                egui::Grid::new(area_id.with("grid2")).num_columns(2).spacing([12.0, 2.0]).show(ui, |ui| {
                    ui.label(s("Ctrl+Z"));
                    ui.label(s("Undo"));
                    ui.end_row();
                    ui.label(s("Ctrl+Click"));
                    ui.label(s("Multi-select"));
                    ui.end_row();
                    ui.label(s("[ / ]"));
                    ui.label(s("Stroke width"));
                    ui.end_row();
                    ui.label(s("Del"));
                    ui.label(s("Delete"));
                    ui.end_row();
                    ui.label(s("Esc"));
                    ui.label(s("Cancel / Deselect"));
                    ui.end_row();
                });
            });
        });
}

/// Draw the annotation selection edit popup (size, color, filled, team, delete).
///
/// Shared between the replay renderer and the tactics board. The popup appears
/// to the right of the selected annotation's screen bounds.
///
/// Call this after checking `single_selected()` and computing `bounds` via
/// [`annotation_screen_bounds`]. The function locks `annotation_arc` internally
/// (the caller must drop any prior lock before calling).
#[allow(clippy::too_many_arguments)]
pub fn draw_annotation_edit_popup(
    ctx: &egui::Context,
    area_id: egui::Id,
    annotation_arc: &Arc<Mutex<AnnotationState>>,
    sel_idx: usize,
    bounds: Rect,
    map_space_size: Option<f32>,
    collab_local_tx: &Option<mpsc::Sender<LocalEvent>>,
    board_id: Option<u64>,
) {
    use crate::icons;

    let popup_pos = Pos2::new(bounds.right() + 8.0, bounds.center().y);
    egui::Area::new(area_id).order(egui::Order::Foreground).fixed_pos(popup_pos).interactable(true).show(ctx, |ui| {
        let frame = egui::Frame::NONE
            .fill(Color32::from_gray(30))
            .corner_radius(egui::CornerRadius::same(6))
            .inner_margin(egui::Margin::same(6))
            .stroke(Stroke::new(1.0, Color32::from_gray(80)));
        frame.show(ui, |ui| {
            let mut ann = annotation_arc.lock();
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
                    let is_circle = matches!(&ann.annotations[sel_idx], Annotation::Circle { .. });
                    let use_km = is_circle && map_space_size.is_some();
                    let mut size = match &ann.annotations[sel_idx] {
                        Annotation::Circle { radius, .. } => *radius,
                        Annotation::Rectangle { half_size, .. } => (half_size.x + half_size.y) / 2.0,
                        Annotation::Triangle { radius, .. } => *radius,
                        _ => 0.0,
                    };
                    let old = size;
                    if use_km {
                        let space_size = map_space_size.unwrap();
                        let bw = size / 768.0 * space_size;
                        let mut km = BigWorldDistance::from(bw).to_km().value();
                        let old_km = km;
                        ui.add(
                            egui::DragValue::new(&mut km).speed(0.1).range(0.1..=20.0).fixed_decimals(1).suffix(" km"),
                        );
                        if km != old_km {
                            size = Km::from(km).to_bigworld().value() / space_size * 768.0;
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
                        send_annotation_update(collab_local_tx, &ann, sel_idx, board_id);
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
                        send_annotation_update(collab_local_tx, &ann, sel_idx, board_id);
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
                    send_annotation_update(collab_local_tx, &ann, sel_idx, board_id);
                }
            }

            // Team toggle (for ships)
            if is_ship && let Annotation::Ship { friendly, .. } = &mut ann.annotations[sel_idx] {
                let (label, color) = if *friendly { ("Friendly", FRIENDLY_COLOR) } else { ("Enemy  ", ENEMY_COLOR) };
                let btn =
                    egui::Button::new(egui::RichText::new(label).color(color).small()).min_size(egui::vec2(60.0, 0.0));
                if ui.add(btn).clicked() {
                    *friendly = !*friendly;
                    send_annotation_update(collab_local_tx, &ann, sel_idx, board_id);
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
                send_annotation_remove(collab_local_tx, id, board_id);
            }
        });
    });
}
